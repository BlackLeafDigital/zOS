// === pages/display.rs — Monitor configuration page ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::gtk::glib;
use std::sync::{Arc, Mutex};

/// A single resolution + refresh rate mode reported by the monitor.
#[derive(Debug, Clone)]
struct Mode {
    width: i64,
    height: i64,
    refresh_rate: f64,
}

impl Mode {
    /// Label shown in the combo: `2560x1440 @ 144Hz`
    fn label(&self) -> String {
        format!(
            "{}x{} @ {:.0}Hz",
            self.width, self.height, self.refresh_rate
        )
    }
}

/// Monitor configuration state shared between UI callbacks and Apply.
struct MonitorConfig {
    name: String,
    width: i64,
    height: i64,
    refresh_rate: f64,
    scale: f64,
    transform: i64,
    x: i64,
    y: i64,
    available_modes: Vec<Mode>,
}

/// Parse an `availableModes` entry like `"2560x1440@143.97Hz"` into a `Mode`.
fn parse_mode_string(s: &str) -> Option<Mode> {
    let s = s.trim().trim_end_matches("Hz");
    let (res, rate) = s.split_once('@')?;
    let (w, h) = res.split_once('x')?;
    Some(Mode {
        width: w.parse().ok()?,
        height: h.parse().ok()?,
        refresh_rate: rate.parse().ok()?,
    })
}

/// Query connected monitors via `hyprctl monitors -j` and parse with serde_json.
fn query_monitors() -> Vec<MonitorConfig> {
    let output = std::process::Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let arr: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let Some(monitors) = arr.as_array() else {
        return Vec::new();
    };

    monitors
        .iter()
        .filter_map(|m| {
            let name = m.get("name")?.as_str()?.to_string();
            let width = m.get("width")?.as_i64()?;
            let height = m.get("height")?.as_i64()?;
            let refresh_rate = m.get("refreshRate")?.as_f64()?;
            let scale = m.get("scale")?.as_f64().unwrap_or(1.0);
            let transform = m.get("transform")?.as_i64().unwrap_or(0);
            let x = m.get("x")?.as_i64().unwrap_or(0);
            let y = m.get("y")?.as_i64().unwrap_or(0);

            let available_modes = m
                .get("availableModes")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|entry| entry.as_str().and_then(parse_mode_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            Some(MonitorConfig {
                name,
                width,
                height,
                refresh_rate,
                scale,
                transform,
                x,
                y,
                available_modes,
            })
        })
        .collect()
}

/// Capture a low-res screenshot of a monitor via grim, returning raw PNG bytes.
fn capture_monitor_png(name: &str) -> Option<Vec<u8>> {
    std::process::Command::new("grim")
        .args(["-o", name, "-t", "png", "-s", "0.2", "-"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| o.stdout)
}

/// Convert PNG bytes to a cairo ImageSurface.
fn png_to_surface(png_bytes: &[u8]) -> Option<gtk::cairo::ImageSurface> {
    use std::io::Cursor;
    gtk::cairo::ImageSurface::create_from_png(&mut Cursor::new(png_bytes)).ok()
}

/// Build the display settings page widget.
pub fn build() -> gtk::Box {
    let page = super::page_content();

    let monitors = query_monitors();
    let shared_configs: Arc<Mutex<Vec<MonitorConfig>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let mut configs = shared_configs.lock().unwrap();
        for mon in &monitors {
            configs.push(MonitorConfig {
                name: mon.name.clone(),
                width: mon.width,
                height: mon.height,
                refresh_rate: mon.refresh_rate,
                scale: mon.scale,
                transform: mon.transform,
                x: mon.x,
                y: mon.y,
                available_modes: mon.available_modes.clone(),
            });
        }
    }

    let canvas = build_monitor_canvas(&shared_configs);
    page.append(&canvas);
    page.append(&build_monitors_section(&shared_configs, &monitors));
    page.append(&build_tools_section());

    super::page_wrapper(&page)
}

// ---------------------------------------------------------------------------
// Monitor canvas
// ---------------------------------------------------------------------------

fn rounded_rect(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
}

fn build_monitor_canvas(configs: &Arc<Mutex<Vec<MonitorConfig>>>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .height_request(250)
        .hexpand(true)
        .build();

    // Capture initial thumbnails for each monitor
    let initial_thumbs: Vec<Option<gtk::cairo::ImageSurface>> = {
        let cfgs = configs.lock().unwrap();
        cfgs.iter()
            .map(|cfg| capture_monitor_png(&cfg.name).and_then(|png| png_to_surface(&png)))
            .collect()
    };
    let thumbnails: Arc<Mutex<Vec<Option<gtk::cairo::ImageSurface>>>> =
        Arc::new(Mutex::new(initial_thumbs));

    let configs_draw = Arc::clone(configs);
    let thumbnails_draw = Arc::clone(&thumbnails);
    area.set_draw_func(move |_area, cr, canvas_w, canvas_h| {
        let cfgs = configs_draw.lock().unwrap();
        if cfgs.is_empty() {
            return;
        }

        let canvas_w = canvas_w as f64;
        let canvas_h = canvas_h as f64;

        // Fill background with Catppuccin Base
        cr.set_source_rgb(
            0x1e as f64 / 255.0,
            0x1e as f64 / 255.0,
            0x2e as f64 / 255.0,
        );
        cr.paint().ok();

        // Calculate bounding box of all monitors
        let mut min_x = i64::MAX;
        let mut min_y = i64::MAX;
        let mut max_x = i64::MIN;
        let mut max_y = i64::MIN;
        for cfg in cfgs.iter() {
            min_x = min_x.min(cfg.x);
            min_y = min_y.min(cfg.y);
            max_x = max_x.max(cfg.x + cfg.width);
            max_y = max_y.max(cfg.y + cfg.height);
        }

        let total_w = (max_x - min_x) as f64;
        let total_h = (max_y - min_y) as f64;
        if total_w <= 0.0 || total_h <= 0.0 {
            return;
        }

        let padding = 32.0;
        let available_w = canvas_w - padding * 2.0;
        let available_h = canvas_h - padding * 2.0;
        let scale = (available_w / total_w).min(available_h / total_h);

        // Center the layout in the canvas
        let offset_x = padding + (available_w - total_w * scale) / 2.0;
        let offset_y = padding + (available_h - total_h * scale) / 2.0;

        let thumbs = thumbnails_draw.lock().unwrap();

        for (i, cfg) in cfgs.iter().enumerate() {
            let rx = offset_x + (cfg.x - min_x) as f64 * scale;
            let ry = offset_y + (cfg.y - min_y) as f64 * scale;
            let rw = cfg.width as f64 * scale;
            let rh = cfg.height as f64 * scale;
            let corner = 8.0;

            // Fill with thumbnail if available, otherwise solid color
            if let Some(Some(ref surface)) = thumbs.get(i) {
                cr.save().ok();
                rounded_rect(cr, rx, ry, rw, rh, corner);
                cr.clip();
                let sw = surface.width() as f64;
                let sh = surface.height() as f64;
                let sx = rw / sw;
                let sy = rh / sh;
                cr.translate(rx, ry);
                cr.scale(sx, sy);
                cr.set_source_surface(surface, 0.0, 0.0).ok();
                cr.paint().ok();
                cr.restore().ok();
            } else {
                // Fallback: Surface0 (#313244)
                cr.set_source_rgb(
                    0x31 as f64 / 255.0,
                    0x32 as f64 / 255.0,
                    0x44 as f64 / 255.0,
                );
                rounded_rect(cr, rx, ry, rw, rh, corner);
                cr.fill().ok();
            }

            // Border: Blue (#89b4fa) for first monitor, Surface1 (#45475a) for others
            if i == 0 {
                cr.set_source_rgb(
                    0x89 as f64 / 255.0,
                    0xb4 as f64 / 255.0,
                    0xfa as f64 / 255.0,
                );
            } else {
                cr.set_source_rgb(
                    0x45 as f64 / 255.0,
                    0x47 as f64 / 255.0,
                    0x5a as f64 / 255.0,
                );
            }
            cr.set_line_width(2.0);
            rounded_rect(cr, rx, ry, rw, rh, corner);
            cr.stroke().ok();

            // Monitor name — Text color (#cdd6f4)
            cr.set_source_rgb(
                0xcd as f64 / 255.0,
                0xd6 as f64 / 255.0,
                0xf4 as f64 / 255.0,
            );
            cr.select_font_face(
                "sans-serif",
                gtk::cairo::FontSlant::Normal,
                gtk::cairo::FontWeight::Bold,
            );
            cr.set_font_size(14.0);
            let name = &cfg.name;
            if let Ok(extents) = cr.text_extents(name) {
                let tx = rx + (rw - extents.width()) / 2.0;
                let ty = ry + rh / 2.0 - 4.0;
                cr.move_to(tx, ty);
                cr.show_text(name).ok();
            }

            // Resolution text — Subtext color (#a6adc8)
            cr.set_source_rgb(
                0xa6 as f64 / 255.0,
                0xad as f64 / 255.0,
                0xc8 as f64 / 255.0,
            );
            cr.select_font_face(
                "sans-serif",
                gtk::cairo::FontSlant::Normal,
                gtk::cairo::FontWeight::Normal,
            );
            cr.set_font_size(11.0);
            let res_text = format!("{}x{}", cfg.width, cfg.height);
            if let Ok(extents) = cr.text_extents(&res_text) {
                let tx = rx + (rw - extents.width()) / 2.0;
                let ty = ry + rh / 2.0 + 14.0;
                cr.move_to(tx, ty);
                cr.show_text(&res_text).ok();
            }
        }
    });

    // Set up periodic thumbnail refresh every 2 seconds
    let canvas_weak = area.downgrade();
    let thumbnails_timer = Arc::clone(&thumbnails);
    let configs_timer = Arc::clone(configs);
    glib::timeout_add_seconds_local(2, move || {
        let Some(canvas) = canvas_weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let cfgs = configs_timer.lock().unwrap();
        let mut thumbs = thumbnails_timer.lock().unwrap();
        for (i, cfg) in cfgs.iter().enumerate() {
            if let Some(png) = capture_monitor_png(&cfg.name) {
                if let Some(surface) = png_to_surface(&png) {
                    if i < thumbs.len() {
                        thumbs[i] = Some(surface);
                    }
                }
            }
        }
        drop(thumbs);
        drop(cfgs);
        canvas.queue_draw();
        glib::ControlFlow::Continue
    });

    area
}

// ---------------------------------------------------------------------------
// Monitors section
// ---------------------------------------------------------------------------

fn build_monitors_section(
    shared_configs: &Arc<Mutex<Vec<MonitorConfig>>>,
    monitors: &[MonitorConfig],
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Displays").build();

    if monitors.is_empty() {
        let empty_row = adw::ActionRow::builder()
            .title("No monitors detected")
            .subtitle("Hyprland may not be running")
            .build();
        group.add(&empty_row);
        return group;
    }

    let scale_options = ["1.0", "1.25", "1.5", "1.75", "2.0"];
    let transform_options = [
        "Normal",
        "90\u{b0}",
        "180\u{b0}",
        "270\u{b0}",
        "Flipped",
        "Flipped 90\u{b0}",
        "Flipped 180\u{b0}",
        "Flipped 270\u{b0}",
    ];

    for (idx, monitor) in monitors.iter().enumerate() {
        let expander = adw::ExpanderRow::builder()
            .title(&monitor.name)
            .subtitle(&format!(
                "{}x{} @ {:.0}Hz (scale {:.2})",
                monitor.width, monitor.height, monitor.refresh_rate, monitor.scale
            ))
            .build();

        // --- Resolution / mode combo ---
        // Build the list of modes from the monitor's reported available modes.
        // Each entry shows "WIDTHxHEIGHT @ RATEHz".
        let modes: Vec<Mode> = if monitor.available_modes.is_empty() {
            // Fallback: just the current mode
            vec![Mode {
                width: monitor.width,
                height: monitor.height,
                refresh_rate: monitor.refresh_rate,
            }]
        } else {
            monitor.available_modes.clone()
        };

        let mode_labels: Vec<String> = modes.iter().map(|m| m.label()).collect();
        let mode_label_refs: Vec<&str> = mode_labels.iter().map(|s| s.as_str()).collect();
        let mode_model = gtk::StringList::new(&mode_label_refs);

        // Find the index of the currently-active mode
        let current_label = format!(
            "{}x{} @ {:.0}Hz",
            monitor.width, monitor.height, monitor.refresh_rate
        );
        let mut mode_idx: u32 = 0;
        for (i, label) in mode_labels.iter().enumerate() {
            if *label == current_label {
                mode_idx = i as u32;
                break;
            }
        }

        let mode_combo = adw::ComboRow::builder()
            .title("Resolution")
            .model(&mode_model)
            .selected(mode_idx)
            .build();

        {
            let configs = Arc::clone(shared_configs);
            let modes_clone = modes.clone();
            mode_combo.connect_selected_notify(move |row| {
                let sel = row.selected() as usize;
                if let Some(mode) = modes_clone.get(sel) {
                    let mut cfgs = configs.lock().unwrap();
                    if let Some(cfg) = cfgs.get_mut(idx) {
                        cfg.width = mode.width;
                        cfg.height = mode.height;
                        cfg.refresh_rate = mode.refresh_rate;
                    }
                }
            });
        }
        expander.add_row(&mode_combo);

        // --- Scale combo ---
        let scale_model = gtk::StringList::new(&scale_options);
        let current_scale_str = format!("{:.1}", monitor.scale);
        let scale_display = if current_scale_str.contains('.') {
            current_scale_str.clone()
        } else {
            format!("{}.0", current_scale_str)
        };
        let mut scale_idx: u32 = 0;
        for (i, opt) in scale_options.iter().enumerate() {
            if *opt == scale_display {
                scale_idx = i as u32;
            }
        }

        let scale_combo = adw::ComboRow::builder()
            .title("Scale")
            .model(&scale_model)
            .selected(scale_idx)
            .build();

        {
            let configs = Arc::clone(shared_configs);
            let s_opts: Vec<String> = scale_options.iter().map(|s| s.to_string()).collect();
            scale_combo.connect_selected_notify(move |row| {
                let sel = row.selected() as usize;
                if let Some(val_str) = s_opts.get(sel) {
                    if let Ok(val) = val_str.parse::<f64>() {
                        let mut cfgs = configs.lock().unwrap();
                        if let Some(cfg) = cfgs.get_mut(idx) {
                            cfg.scale = val;
                        }
                    }
                }
            });
        }
        expander.add_row(&scale_combo);

        // --- Transform combo ---
        let transform_model = gtk::StringList::new(&transform_options);
        let transform_idx = (monitor.transform as u32).min(7);
        let transform_combo = adw::ComboRow::builder()
            .title("Rotation")
            .model(&transform_model)
            .selected(transform_idx)
            .build();

        {
            let configs = Arc::clone(shared_configs);
            transform_combo.connect_selected_notify(move |row| {
                let sel = row.selected() as i64;
                let mut cfgs = configs.lock().unwrap();
                if let Some(cfg) = cfgs.get_mut(idx) {
                    cfg.transform = sel;
                }
            });
        }
        expander.add_row(&transform_combo);

        // --- Position X ---
        let pos_x_row = adw::ActionRow::builder().title("Position X").build();
        let pos_x_spin = gtk::SpinButton::builder()
            .adjustment(&gtk::Adjustment::new(
                monitor.x as f64,
                -10000.0,
                10000.0,
                10.0,
                100.0,
                0.0,
            ))
            .valign(gtk::Align::Center)
            .build();
        {
            let configs = Arc::clone(shared_configs);
            pos_x_spin.connect_value_changed(move |spin| {
                let mut cfgs = configs.lock().unwrap();
                if let Some(cfg) = cfgs.get_mut(idx) {
                    cfg.x = spin.value() as i64;
                }
            });
        }
        pos_x_row.add_suffix(&pos_x_spin);
        expander.add_row(&pos_x_row);

        // --- Position Y ---
        let pos_y_row = adw::ActionRow::builder().title("Position Y").build();
        let pos_y_spin = gtk::SpinButton::builder()
            .adjustment(&gtk::Adjustment::new(
                monitor.y as f64,
                -10000.0,
                10000.0,
                10.0,
                100.0,
                0.0,
            ))
            .valign(gtk::Align::Center)
            .build();
        {
            let configs = Arc::clone(shared_configs);
            pos_y_spin.connect_value_changed(move |spin| {
                let mut cfgs = configs.lock().unwrap();
                if let Some(cfg) = cfgs.get_mut(idx) {
                    cfg.y = spin.value() as i64;
                }
            });
        }
        pos_y_row.add_suffix(&pos_y_spin);
        expander.add_row(&pos_y_row);

        group.add(&expander);
    }

    // --- Apply button ---
    let apply_row = adw::ActionRow::builder()
        .title("Apply Changes")
        .subtitle("Write monitor config and reload Hyprland")
        .build();

    let apply_btn = gtk::Button::builder()
        .label("Apply")
        .valign(gtk::Align::Center)
        .css_classes(["suggested-action"])
        .build();

    {
        let configs = Arc::clone(shared_configs);
        apply_btn.connect_clicked(move |_| {
            let cfgs = configs.lock().unwrap();
            let home = std::env::var("HOME").unwrap_or_default();
            let conf_dir = format!("{}/.config/hypr", home);
            if let Err(e) = std::fs::create_dir_all(&conf_dir) {
                tracing::error!("Failed to create {}: {}", conf_dir, e);
                return;
            }
            let path = format!("{}/monitors.conf", conf_dir);

            let mut content =
                String::from("# zOS Monitor Config \u{2014} managed by zos-settings\n");
            for cfg in cfgs.iter() {
                content.push_str(&format!(
                    "monitor={},{}x{}@{:.2},{}x{},{:.2},transform,{}\n",
                    cfg.name,
                    cfg.width,
                    cfg.height,
                    cfg.refresh_rate,
                    cfg.x,
                    cfg.y,
                    cfg.scale,
                    cfg.transform
                ));
            }

            match std::fs::write(&path, &content) {
                Ok(()) => tracing::info!("Wrote monitor config to {}", path),
                Err(e) => tracing::error!("Failed to write monitor config: {}", e),
            }

            crate::services::hyprctl::reload();
            tracing::info!("Hyprland config reloaded");
        });
    }

    apply_row.add_suffix(&apply_btn);
    apply_row.set_activatable_widget(Some(&apply_btn));
    group.add(&apply_row);

    group
}

// ---------------------------------------------------------------------------
// Tools section
// ---------------------------------------------------------------------------

fn build_tools_section() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Tools").build();

    let nwg_row = adw::ActionRow::builder()
        .title("Open nwg-displays")
        .subtitle("Drag-to-arrange monitor layout")
        .build();

    let nwg_btn = gtk::Button::builder()
        .label("Open")
        .valign(gtk::Align::Center)
        .build();

    nwg_btn.connect_clicked(
        |_| match std::process::Command::new("nwg-displays").spawn() {
            Ok(_) => tracing::info!("Launched nwg-displays"),
            Err(e) => tracing::error!("Failed to launch nwg-displays: {}", e),
        },
    );

    nwg_row.add_suffix(&nwg_btn);
    nwg_row.set_activatable_widget(Some(&nwg_btn));
    group.add(&nwg_row);

    group
}
