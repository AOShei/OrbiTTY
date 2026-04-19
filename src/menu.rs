use adw::prelude::*;
use gtk::gio;
use gtk4 as gtk;
use libadwaita as adw;

use crate::app;

pub fn build_main_menu_button() -> gtk::MenuButton {
    let btn = gtk::MenuButton::new();
    btn.set_icon_name("open-menu-symbolic");
    btn.set_tooltip_text(Some("Main Menu"));
    btn.set_primary(true);

    let model = build_menu_model();
    let popover = gtk::PopoverMenu::from_model(Some(&model));

    popover.add_child(&build_theme_row(), "theme");
    popover.add_child(&build_zoom_row(), "zoom");

    btn.set_popover(Some(&popover));
    btn
}

fn build_menu_model() -> gio::Menu {
    let model = gio::Menu::new();

    let custom = gio::Menu::new();
    let theme_item = gio::MenuItem::new(None, None);
    theme_item.set_attribute_value("custom", Some(&"theme".to_variant()));
    custom.append_item(&theme_item);
    let zoom_item = gio::MenuItem::new(None, None);
    zoom_item.set_attribute_value("custom", Some(&"zoom".to_variant()));
    custom.append_item(&zoom_item);
    model.append_section(None, &custom);

    let mid = gio::Menu::new();
    mid.append(Some("New _Terminal"), Some("win.new-session"));
    mid.append(Some("_New Window"), Some("app.new-window"));
    mid.append(Some("_Show All Tabs"), Some("win.tab-overview"));
    mid.append(Some("_Fullscreen"), Some("win.fullscreen"));
    model.append_section(None, &mid);

    let end = gio::Menu::new();
    end.append(Some("_Preferences"), Some("app.preferences"));
    end.append(Some("_Keyboard Shortcuts"), Some("app.shortcuts"));
    end.append(Some("_About OrbiTTY"), Some("app.about"));
    model.append_section(None, &end);

    model
}

fn build_theme_row() -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 18);
    row.set_halign(gtk::Align::Center);
    row.set_margin_top(10);
    row.set_margin_bottom(6);
    row.set_margin_start(12);
    row.set_margin_end(12);

    let sm = adw::StyleManager::default();

    let system_btn = gtk::ToggleButton::new();
    system_btn.add_css_class("orbit-theme-choice");
    system_btn.add_css_class("system");
    system_btn.set_tooltip_text(Some("Follow System"));

    let light_btn = gtk::ToggleButton::new();
    light_btn.add_css_class("orbit-theme-choice");
    light_btn.add_css_class("light");
    light_btn.set_tooltip_text(Some("Light"));
    light_btn.set_group(Some(&system_btn));

    let dark_btn = gtk::ToggleButton::new();
    dark_btn.add_css_class("orbit-theme-choice");
    dark_btn.add_css_class("dark");
    dark_btn.set_tooltip_text(Some("Dark"));
    dark_btn.set_group(Some(&system_btn));

    match sm.color_scheme() {
        adw::ColorScheme::ForceLight | adw::ColorScheme::PreferLight => {
            light_btn.set_active(true);
        }
        adw::ColorScheme::ForceDark | adw::ColorScheme::PreferDark => {
            dark_btn.set_active(true);
        }
        _ => {
            system_btn.set_active(true);
        }
    }

    {
        let sm = sm.clone();
        system_btn.connect_toggled(move |b| {
            if b.is_active() {
                sm.set_color_scheme(adw::ColorScheme::Default);
            }
        });
    }
    {
        let sm = sm.clone();
        light_btn.connect_toggled(move |b| {
            if b.is_active() {
                sm.set_color_scheme(adw::ColorScheme::ForceLight);
            }
        });
    }
    {
        let sm = sm.clone();
        dark_btn.connect_toggled(move |b| {
            if b.is_active() {
                sm.set_color_scheme(adw::ColorScheme::ForceDark);
            }
        });
    }

    let mk_col = |btn: &gtk::ToggleButton, label: &str| {
        let col = gtk::Box::new(gtk::Orientation::Vertical, 6);
        col.set_halign(gtk::Align::Center);
        col.append(btn);
        let lbl = gtk::Label::new(Some(label));
        lbl.add_css_class("caption");
        col.append(&lbl);
        col
    };

    row.append(&mk_col(&system_btn, "System"));
    row.append(&mk_col(&light_btn, "Light"));
    row.append(&mk_col(&dark_btn, "Dark"));

    row
}

fn build_zoom_row() -> gtk::Box {
    let outer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    outer.set_halign(gtk::Align::Center);
    outer.set_margin_top(4);
    outer.set_margin_bottom(4);
    outer.set_margin_start(10);
    outer.set_margin_end(10);

    let group = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    group.add_css_class("linked");

    let out_btn = gtk::Button::from_icon_name("zoom-out-symbolic");
    out_btn.set_action_name(Some("app.zoom-out"));
    out_btn.set_tooltip_text(Some("Zoom Out"));

    let label_btn = gtk::Button::with_label("100%");
    label_btn.set_action_name(Some("app.zoom-reset"));
    label_btn.set_tooltip_text(Some("Reset Zoom"));
    label_btn.set_width_request(64);

    let in_btn = gtk::Button::from_icon_name("zoom-in-symbolic");
    in_btn.set_action_name(Some("app.zoom-in"));
    in_btn.set_tooltip_text(Some("Zoom In"));

    group.append(&out_btn);
    group.append(&label_btn);
    group.append(&in_btn);

    outer.append(&group);

    let pct = (app::current_font_scale() * 100.0).round() as i32;
    label_btn.set_label(&format!("{pct}%"));
    app::register_zoom_label(label_btn);

    outer
}
