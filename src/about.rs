use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

pub fn show(parent: Option<&gtk::Window>) {
    let about = adw::AboutWindow::builder()
        .application_name("Orbitty")
        .application_icon("utilities-terminal")
        .developer_name("Orbitty Contributors")
        .version(env!("CARGO_PKG_VERSION"))
        .license_type(gtk::License::MitX11)
        .comments("Workspace-based, tiling GTK4 terminal for async tasks.")
        .build();
    if let Some(p) = parent {
        about.set_transient_for(Some(p));
    }
    about.present();
}
