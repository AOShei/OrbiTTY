use gtk::prelude::*;
use gtk4 as gtk;

pub fn show(parent: Option<&gtk::Window>) {
    let builder = gtk::Builder::from_string(include_str!("shortcuts.ui"));
    let Some(win) = builder.object::<gtk::ShortcutsWindow>("shortcuts") else {
        eprintln!("[orbit] failed to load shortcuts.ui");
        return;
    };
    if let Some(p) = parent {
        win.set_transient_for(Some(p));
    }
    win.present();
}
