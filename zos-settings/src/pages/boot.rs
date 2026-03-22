// === pages/boot.rs — Boot/GRUB configuration page ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use zos_core::commands::grub;

/// Build the boot configuration page widget.
pub fn build() -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    let status = grub::get_grub_status();
    let is_root = grub::is_root();

    // --- Warning Banner ---
    if !is_root {
        let warning_group = adw::PreferencesGroup::builder().build();
        let warning_row = adw::ActionRow::builder()
            .title("Some changes require administrator privileges. Run with sudo for full access.")
            .build();
        let warning_icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
        warning_icon.set_valign(gtk::Align::Center);
        warning_row.add_prefix(&warning_icon);
        warning_group.add(&warning_row);
        page.append(&warning_group);
    }

    // --- GRUB Settings ---
    let boot_group = adw::PreferencesGroup::builder().title("Boot").build();

    let timeout_val = status.current_timeout.unwrap_or(0) as f64;
    let timeout_row = adw::ActionRow::builder()
        .title("GRUB Timeout (seconds)")
        .subtitle("Time to wait at boot menu before auto-selecting default")
        .build();
    let timeout_spin = gtk::SpinButton::with_range(0.0, 30.0, 1.0);
    timeout_spin.set_value(timeout_val);
    timeout_spin.set_valign(gtk::Align::Center);
    timeout_row.add_suffix(&timeout_spin);
    boot_group.add(&timeout_row);

    let apply_row = adw::ActionRow::builder()
        .title("Apply Timeout")
        .subtitle("Write timeout value to GRUB configuration")
        .build();
    let apply_btn = gtk::Button::builder()
        .label("Apply")
        .valign(gtk::Align::Center)
        .css_classes(["suggested-action"])
        .build();

    let timeout_spin_clone = timeout_spin.clone();
    apply_btn.connect_clicked(move |btn| {
        let val = timeout_spin_clone.value() as u32;
        match grub::apply_grub_timeout(val) {
            Ok(()) => {
                tracing::info!("GRUB timeout set to {}", val);
                btn.set_label("Applied");
                btn.set_sensitive(false);
            }
            Err(e) => {
                tracing::error!("Failed to apply GRUB timeout: {}", e);
                btn.set_label("Error");
            }
        }
    });
    apply_row.add_suffix(&apply_btn);
    apply_row.set_activatable_widget(Some(&apply_btn));
    boot_group.add(&apply_row);

    let root_row = adw::ActionRow::builder()
        .title("Running as root")
        .subtitle(if is_root { "Yes" } else { "No" })
        .build();
    let root_icon = gtk::Image::from_icon_name(if is_root {
        "emblem-ok-symbolic"
    } else {
        "dialog-warning-symbolic"
    });
    root_icon.set_valign(gtk::Align::Center);
    root_row.add_suffix(&root_icon);
    boot_group.add(&root_row);

    page.append(&boot_group);

    // --- Dual Boot / Windows ---
    let windows_group = adw::PreferencesGroup::builder().title("Windows").build();

    let detected_row = adw::ActionRow::builder()
        .title("Windows Detected")
        .subtitle(if status.windows_detected {
            status.windows_path.as_deref().unwrap_or("Yes")
        } else {
            "No"
        })
        .build();
    windows_group.add(&detected_row);

    let bls_row = adw::ActionRow::builder()
        .title("Boot Loader Entry")
        .subtitle(if status.bls_entry_exists {
            "Present"
        } else {
            "Not configured"
        })
        .build();
    windows_group.add(&bls_row);

    if status.windows_detected && !status.bls_entry_exists {
        let create_row = adw::ActionRow::builder()
            .title("Create Windows Boot Entry")
            .subtitle("Add a BLS entry for Windows dual-boot")
            .build();
        let create_btn = gtk::Button::builder()
            .label("Create")
            .valign(gtk::Align::Center)
            .css_classes(["suggested-action"])
            .build();
        create_btn.connect_clicked(move |btn| match grub::create_windows_bls() {
            Ok(()) => {
                tracing::info!("Windows BLS entry created");
                btn.set_label("Created");
                btn.set_sensitive(false);
            }
            Err(e) => {
                tracing::error!("Failed to create Windows BLS entry: {}", e);
                btn.set_label("Error");
            }
        });
        create_row.add_suffix(&create_btn);
        create_row.set_activatable_widget(Some(&create_btn));
        windows_group.add(&create_row);
    }

    page.append(&windows_group);

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(&page)
        .build();

    let wrapper = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    wrapper.append(&scrolled);
    wrapper
}
