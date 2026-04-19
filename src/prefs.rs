use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

pub fn show(parent: Option<&gtk::Window>) {
    let win = adw::PreferencesWindow::builder()
        .title("Preferences")
        .modal(true)
        .build();
    if let Some(p) = parent {
        win.set_transient_for(Some(p));
    }

    let page = adw::PreferencesPage::builder()
        .title("General")
        .icon_name("preferences-system-symbolic")
        .build();

    let group = adw::PreferencesGroup::builder().title("General").build();
    let row = adw::ActionRow::builder()
        .title("Coming Soon")
        .subtitle("Preferences will be added in a future release.")
        .build();
    group.add(&row);

    page.add(&group);
    win.add(&page);
    win.present();
}
