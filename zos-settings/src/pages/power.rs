// === pages/power.rs — Power management page ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

/// Show a confirmation dialog before executing a power action.
fn confirm_power_action(btn: &gtk::Button, title: &str, body: &str, action: fn()) {
    let window = btn.root().and_then(|r| r.downcast::<gtk::Window>().ok());

    let dialog = adw::AlertDialog::builder()
        .heading(title)
        .body(body)
        .build();

    dialog.add_responses(&[("cancel", "Cancel"), ("confirm", "Confirm")]);
    dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, move |_, response| {
        if response == "confirm" {
            action();
        }
    });

    if let Some(ref w) = window {
        dialog.present(Some(w));
    } else {
        dialog.present(None::<&gtk::Window>);
    }
}

/// Build the power management page widget.
pub fn build() -> gtk::Box {
    let page = super::page_content();

    let power_group = adw::PreferencesGroup::builder()
        .title("Power")
        .description("System power actions")
        .build();

    // --- Suspend ---
    let suspend_row = adw::ActionRow::builder()
        .title("Suspend")
        .subtitle("Put the system to sleep")
        .build();
    let suspend_icon = gtk::Image::from_icon_name("media-playback-pause-symbolic");
    suspend_icon.set_valign(gtk::Align::Center);
    suspend_row.add_prefix(&suspend_icon);
    let suspend_btn = gtk::Button::builder()
        .label("Suspend")
        .valign(gtk::Align::Center)
        .build();
    suspend_btn.connect_clicked(|btn| {
        confirm_power_action(
            btn,
            "Suspend?",
            "The system will be put to sleep.",
            crate::services::power::suspend,
        );
    });
    suspend_row.add_suffix(&suspend_btn);
    suspend_row.set_activatable_widget(Some(&suspend_btn));
    power_group.add(&suspend_row);

    // --- Reboot ---
    let reboot_row = adw::ActionRow::builder()
        .title("Reboot")
        .subtitle("Restart the system")
        .build();
    let reboot_icon = gtk::Image::from_icon_name("view-refresh-symbolic");
    reboot_icon.set_valign(gtk::Align::Center);
    reboot_row.add_prefix(&reboot_icon);
    let reboot_btn = gtk::Button::builder()
        .label("Reboot")
        .valign(gtk::Align::Center)
        .build();
    reboot_btn.connect_clicked(|btn| {
        confirm_power_action(
            btn,
            "Reboot?",
            "The system will restart. All unsaved work will be lost.",
            crate::services::power::reboot,
        );
    });
    reboot_row.add_suffix(&reboot_btn);
    reboot_row.set_activatable_widget(Some(&reboot_btn));
    power_group.add(&reboot_row);

    // --- Shut Down ---
    let shutdown_row = adw::ActionRow::builder()
        .title("Shut Down")
        .subtitle("Power off the system")
        .build();
    let shutdown_icon = gtk::Image::from_icon_name("system-shutdown-symbolic");
    shutdown_icon.set_valign(gtk::Align::Center);
    shutdown_row.add_prefix(&shutdown_icon);
    let shutdown_btn = gtk::Button::builder()
        .label("Shut Down")
        .valign(gtk::Align::Center)
        .css_classes(["destructive-action"])
        .build();
    shutdown_btn.connect_clicked(|btn| {
        confirm_power_action(
            btn,
            "Shut Down?",
            "All unsaved work will be lost.",
            crate::services::power::shutdown,
        );
    });
    shutdown_row.add_suffix(&shutdown_btn);
    shutdown_row.set_activatable_widget(Some(&shutdown_btn));
    power_group.add(&shutdown_row);

    page.append(&power_group);
    super::page_wrapper(&page)
}
