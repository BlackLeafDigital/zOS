use relm4::gtk;
use relm4::gtk::prelude::*;

pub mod appearance;
pub mod audio;
pub mod boot;
pub mod display;
pub mod dock;
pub mod input;
pub mod network;
pub mod overview;
pub mod power;

/// Create a standard page content box with consistent spacing and margins.
pub fn page_content() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build()
}

/// Wrap a page content box in a scrolled window.
pub fn page_wrapper(content: &gtk::Box) -> gtk::Box {
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(content)
        .build();

    let wrapper = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    wrapper.append(&scrolled);
    wrapper
}
