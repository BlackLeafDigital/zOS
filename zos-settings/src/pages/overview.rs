// === pages/overview.rs — Overview/Dashboard page ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use zos_core::commands::doctor::{self, CheckStatus};
use zos_core::commands::status;

/// Build the overview page widget.
pub fn build() -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    // --- System Info Section ---
    let info = status::get_system_info();

    let info_group = adw::PreferencesGroup::builder()
        .title("System Information")
        .build();

    let os_row = adw::ActionRow::builder()
        .title("OS Version")
        .subtitle(&info.os_version)
        .build();
    info_group.add(&os_row);

    let image_row = adw::ActionRow::builder()
        .title("Image")
        .subtitle(&info.image_name)
        .build();
    info_group.add(&image_row);

    let fedora_row = adw::ActionRow::builder()
        .title("Fedora Version")
        .subtitle(&info.fedora_version)
        .build();
    info_group.add(&fedora_row);

    let update_row = adw::ActionRow::builder()
        .title("Last Update")
        .subtitle(&info.last_update)
        .build();
    info_group.add(&update_row);

    page.append(&info_group);

    // --- Health Check Section ---
    let checks = doctor::run_doctor_checks();
    let (pass, fail, warn) = doctor::summarize(&checks);

    let health_group = adw::PreferencesGroup::builder()
        .title("Health Checks")
        .description(format!(
            "{} passed, {} warnings, {} failed",
            pass, warn, fail
        ))
        .build();

    for check in &checks {
        let row = adw::ActionRow::builder()
            .title(&check.name)
            .subtitle(&check.message)
            .build();

        let status_label = gtk::Label::builder()
            .label(&check.status.to_string())
            .valign(gtk::Align::Center)
            .build();

        let css_class = match check.status {
            CheckStatus::Pass => "health-pass",
            CheckStatus::Warn => "health-warn",
            CheckStatus::Fail => "health-fail",
        };
        status_label.add_css_class(css_class);

        row.add_suffix(&status_label);

        health_group.add(&row);
    }

    page.append(&health_group);

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
