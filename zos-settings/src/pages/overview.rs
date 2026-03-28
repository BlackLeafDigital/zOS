// === pages/overview.rs — Dashboard overview page ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

use zos_core::commands::doctor::{self, CheckStatus};
use zos_core::commands::migrate;
use zos_core::commands::setup;
use zos_core::commands::status;
use zos_core::commands::update;
use zos_core::config;

/// Build the overview dashboard page widget.
pub fn build() -> gtk::Box {
    let page = super::page_content();

    // Gather data upfront
    let info = status::get_system_info();
    let checks = doctor::run_doctor_checks();
    let (pass, fail, warn) = doctor::summarize(&checks);
    let setup_steps = setup::get_setup_steps();
    let update_status = update::check_for_updates();
    let config_areas = status::get_config_status();
    let migrations = migrate::plan_migrations();

    // --- 1. Welcome Header ---
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_bottom(8)
        .build();

    let greeting = gtk::Label::builder()
        .label("Welcome to zOS")
        .halign(gtk::Align::Start)
        .build();
    greeting.add_css_class("dashboard-greeting");

    let subtitle_text = format!(
        "{} {} (Fedora {})",
        info.image_name, info.os_version, info.fedora_version
    );
    let subtitle = gtk::Label::builder()
        .label(&subtitle_text)
        .halign(gtk::Align::Start)
        .build();
    subtitle.add_css_class("dashboard-subtitle");

    header.append(&greeting);
    header.append(&subtitle);
    page.append(&header);

    // --- 2. Status Summary Cards ---
    let cards_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(16)
        .homogeneous(true)
        .build();

    // Card 1: Health
    let health_value = pass.to_string();
    cards_box.append(&crate::widgets::stat_card(
        &health_value,
        "dashboard-stat-value-green",
        "checks passed",
    ));

    // Card 2: Setup
    let installed_count = setup_steps.iter().filter(|s| s.installed).count();
    let total_count = setup_steps.len();
    let setup_value = format!("{}/{}", installed_count, total_count);
    cards_box.append(&crate::widgets::stat_card(
        &setup_value,
        "dashboard-stat-value",
        "tools ready",
    ));

    // Card 3: Updates
    match &update_status {
        Ok(us) if us.pending => {
            cards_box.append(&crate::widgets::stat_card(
                "1",
                "dashboard-stat-value-purple",
                "update available",
            ));
        }
        Ok(_) => {
            cards_box.append(&crate::widgets::stat_card(
                "\u{2713}",
                "dashboard-stat-value-green",
                "up to date",
            ));
        }
        Err(_) => {
            cards_box.append(&crate::widgets::stat_card(
                "?",
                "dashboard-stat-value-yellow",
                "unknown",
            ));
        }
    }

    // Card 4: Config
    let configs_up_to_date = config_areas.iter().filter(|c| c.up_to_date).count();
    let configs_total = config_areas.len();
    let config_value = format!("{}/{}", configs_up_to_date, configs_total);
    let config_class = if configs_up_to_date == configs_total {
        "dashboard-stat-value-green"
    } else {
        "dashboard-stat-value-yellow"
    };
    cards_box.append(&crate::widgets::stat_card(
        &config_value,
        config_class,
        "configs current",
    ));

    page.append(&cards_box);

    // --- 3. System Information ---
    let info_group = adw::PreferencesGroup::builder()
        .title("System Information")
        .build();

    let os_row = adw::ActionRow::builder()
        .title("OS Version")
        .subtitle(&info.os_version)
        .build();
    os_row.add_prefix(&gtk::Image::from_icon_name("emblem-system-symbolic"));
    info_group.add(&os_row);

    let image_row = adw::ActionRow::builder()
        .title("Image")
        .subtitle(&info.image_name)
        .build();
    image_row.add_prefix(&gtk::Image::from_icon_name("drive-harddisk-symbolic"));
    info_group.add(&image_row);

    let fedora_row = adw::ActionRow::builder()
        .title("Fedora Version")
        .subtitle(&info.fedora_version)
        .build();
    fedora_row.add_prefix(&gtk::Image::from_icon_name(
        "system-software-install-symbolic",
    ));
    info_group.add(&fedora_row);

    let update_row = adw::ActionRow::builder()
        .title("Last Update")
        .subtitle(&info.last_update)
        .build();
    update_row.add_prefix(&gtk::Image::from_icon_name("appointment-soon-symbolic"));
    info_group.add(&update_row);

    page.append(&info_group);

    // --- 4. Updates Section ---
    let updates_group = adw::PreferencesGroup::builder().title("Updates").build();

    match &update_status {
        Ok(us) => {
            // Current image row
            let current_row = adw::ActionRow::builder()
                .title("Current Image")
                .subtitle(&us.current_image)
                .build();
            updates_group.add(&current_row);

            if us.pending {
                let status_row = adw::ActionRow::builder().title("Update available").build();

                let status_label = gtk::Label::builder()
                    .label("Update available")
                    .valign(gtk::Align::Center)
                    .build();
                status_label.add_css_class("dashboard-stat-value-purple");

                let apply_btn = gtk::Button::builder()
                    .label("Apply Update")
                    .valign(gtk::Align::Center)
                    .build();
                apply_btn.add_css_class("suggested-action");

                apply_btn.connect_clicked(move |btn| {
                    btn.set_sensitive(false);
                    btn.set_label("Applying...");
                    std::thread::spawn(|| {
                        let _ = update::apply_update();
                    });
                });

                status_row.add_suffix(&apply_btn);
                updates_group.add(&status_row);
            } else {
                let status_row = adw::ActionRow::builder()
                    .title("System is up to date")
                    .build();

                let check_icon = gtk::Image::from_icon_name("emblem-default-symbolic");
                check_icon.add_css_class("health-pass");
                status_row.add_suffix(&check_icon);

                updates_group.add(&status_row);
            }
        }
        Err(e) => {
            let error_row = adw::ActionRow::builder()
                .title("Could not check for updates")
                .subtitle(&e.to_string())
                .build();

            let warn_icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
            warn_icon.add_css_class("health-warn");
            error_row.add_prefix(&warn_icon);

            updates_group.add(&error_row);
        }
    }

    page.append(&updates_group);

    // --- 5. Setup Progress (only if setup is not done) ---
    if !config::is_setup_done() {
        let setup_group = adw::PreferencesGroup::builder()
            .title("Setup")
            .description("Developer tools")
            .build();

        for step in &setup_steps {
            let row = adw::ActionRow::builder()
                .title(&step.name)
                .subtitle(&step.description)
                .build();

            if step.installed {
                let check_icon = gtk::Image::from_icon_name("emblem-default-symbolic");
                check_icon.add_css_class("health-pass");
                row.add_suffix(&check_icon);
            } else {
                let install_cmd = step.install_cmd.clone();
                let install_btn = gtk::Button::builder()
                    .label("Install")
                    .valign(gtk::Align::Center)
                    .build();
                install_btn.add_css_class("suggested-action");

                install_btn.connect_clicked(move |btn| {
                    btn.set_sensitive(false);
                    btn.set_label("Installing...");
                    let cmd = install_cmd.clone();
                    std::thread::spawn(move || {
                        if !cmd.is_empty() {
                            let (program, args) = cmd.split_first().unwrap();
                            let _ = std::process::Command::new(program).args(args).status();
                        }
                    });
                });

                row.add_suffix(&install_btn);
            }

            setup_group.add(&row);
        }

        page.append(&setup_group);
    }

    // --- 6. Pending Migrations (only if non-empty) ---
    if !migrations.is_empty() {
        let migrate_group = adw::PreferencesGroup::builder()
            .title("Config Updates Available")
            .build();

        for action in &migrations {
            let row = adw::ActionRow::builder()
                .title(&action.area)
                .subtitle(&action.description)
                .build();
            migrate_group.add(&row);
        }

        // "Apply All" button row
        let apply_all_btn = gtk::Button::builder()
            .label("Apply All")
            .halign(gtk::Align::Center)
            .margin_top(8)
            .build();
        apply_all_btn.add_css_class("suggested-action");

        let migrations_clone = migrations.clone();
        apply_all_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            btn.set_label("Applying...");
            let mut actions = migrations_clone.clone();
            std::thread::spawn(move || {
                let _ = migrate::apply_migrations(&mut actions);
            });
        });

        // Wrap button in a box since PreferencesGroup only accepts ListBoxRow-like children
        let btn_row = adw::ActionRow::builder().build();
        btn_row.add_suffix(&apply_all_btn);
        migrate_group.add(&btn_row);

        page.append(&migrate_group);
    }

    // --- 7. Health Checks ---
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

        // Prefix icon
        let (icon_name, icon_class) = match check.status {
            CheckStatus::Pass => ("emblem-default-symbolic", "health-pass"),
            CheckStatus::Warn => ("dialog-warning-symbolic", "health-warn"),
            CheckStatus::Fail => ("process-stop-symbolic", "health-fail"),
        };
        let prefix_icon = gtk::Image::from_icon_name(icon_name);
        prefix_icon.add_css_class(icon_class);
        row.add_prefix(&prefix_icon);

        // Suffix status label
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

    super::page_wrapper(&page)
}
