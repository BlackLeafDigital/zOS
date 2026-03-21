// === pages/power.rs — Power management page ===

use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;

/// Show a confirmation dialog before executing a power action.
fn confirm_power_action(btn: &gtk::Button, title: &str, body: &str, action: fn()) {
    let window = btn
        .root()
        .and_then(|r| r.downcast::<gtk::Window>().ok());

    #[allow(deprecated)]
    let dialog = gtk::MessageDialog::builder()
        .message_type(gtk::MessageType::Warning)
        .buttons(gtk::ButtonsType::OkCancel)
        .text(title)
        .secondary_text(body)
        .modal(true)
        .build();

    if let Some(ref w) = window {
        dialog.set_transient_for(Some(w));
    }

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Ok {
            action();
        }
        dialog.close();
    });

    dialog.present();
}

/// Build the power management page widget.
pub fn build() -> gtk::Box {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    let power_group = adw::PreferencesGroup::builder()
        .title("Power")
        .description("System power actions")
        .build();

    // --- Suspend ---
    let suspend_row = adw::ActionRow::builder()
        .title("Suspend")
        .subtitle("Put the system to sleep")
        .build();
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
    page
}
