// === pages/devsetup.rs — Developer setup page ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use zos_core::commands::setup;
use zos_core::config;

/// Build the developer setup page widget.
pub fn build() -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    // --- Status Banner ---
    let steps = setup::get_setup_steps();
    let pending: usize = steps.iter().filter(|s| !s.installed).count();

    let banner_text = if config::is_setup_done() {
        "First-login setup complete".to_string()
    } else {
        format!(
            "Setup incomplete — {} step{} remaining",
            pending,
            if pending == 1 { "" } else { "s" }
        )
    };

    let banner_group = adw::PreferencesGroup::builder().build();
    let banner_row = adw::ActionRow::builder()
        .title(&banner_text)
        .build();
    let banner_icon = gtk::Image::from_icon_name(if config::is_setup_done() {
        "emblem-ok-symbolic"
    } else {
        "dialog-warning-symbolic"
    });
    banner_icon.set_valign(gtk::Align::Center);
    banner_row.add_prefix(&banner_icon);
    banner_group.add(&banner_row);
    page.append(&banner_group);

    // --- Setup Steps ---
    let steps_group = adw::PreferencesGroup::builder()
        .title("Developer Tools")
        .build();

    for step in &steps {
        let row = adw::ActionRow::builder()
            .title(step.name.as_str())
            .subtitle(step.description.as_str())
            .build();

        if step.installed {
            let check = gtk::Image::from_icon_name("emblem-ok-symbolic");
            check.set_valign(gtk::Align::Center);
            check.add_css_class("success");
            row.add_suffix(&check);
        } else {
            let install_btn = gtk::Button::builder()
                .label("Install")
                .valign(gtk::Align::Center)
                .build();

            let step_clone = step.clone();
            let btn_ref = install_btn.clone();
            install_btn.connect_clicked(move |_| {
                btn_ref.set_sensitive(false);
                btn_ref.set_label("Installing…");

                match setup::run_setup_step(&step_clone) {
                    Ok(()) => {
                        tracing::info!("Setup step '{}' completed", step_clone.name);
                        // Replace button with checkmark
                        if let Some(parent_row) = btn_ref
                            .parent()
                            .and_then(|p| p.downcast::<adw::ActionRow>().ok())
                        {
                            parent_row.remove(&btn_ref);
                            let check = gtk::Image::from_icon_name("emblem-ok-symbolic");
                            check.set_valign(gtk::Align::Center);
                            check.add_css_class("success");
                            parent_row.add_suffix(&check);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Setup step '{}' failed: {}", step_clone.name, e);
                        btn_ref.set_label("Retry");
                        btn_ref.set_sensitive(true);
                    }
                }
            });

            row.add_suffix(&install_btn);
        }

        steps_group.add(&row);
    }

    page.append(&steps_group);

    // --- Run All Pending Button ---
    if pending > 0 {
        let run_all_btn = gtk::Button::builder()
            .label("Run All Pending")
            .halign(gtk::Align::Center)
            .css_classes(["suggested-action", "pill"])
            .build();

        let steps_for_all: Vec<setup::SetupStep> =
            steps.into_iter().filter(|s| !s.installed).collect();

        run_all_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            btn.set_label("Running…");

            for step in &steps_for_all {
                match setup::run_setup_step(step) {
                    Ok(()) => {
                        tracing::info!("Setup step '{}' completed", step.name);
                    }
                    Err(e) => {
                        tracing::error!("Setup step '{}' failed: {}", step.name, e);
                    }
                }
            }

            btn.set_label("Done");
        });

        page.append(&run_all_btn);
    }

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
