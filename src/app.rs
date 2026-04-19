use adw::prelude::*;
use gtk::gdk;
use gtk4 as gtk;
use libadwaita as adw;

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use crate::window::OrbitWindow;

const APP_ID: &str = "dev.orbit.Orbit";

pub struct AppState {
    pub font_scale: f64,
    pub windows: Vec<Weak<OrbitWindow>>,
    pub zoom_labels: Vec<glib::WeakRef<gtk::Button>>,
}

thread_local! {
    pub static STATE: RefCell<AppState> = RefCell::new(AppState {
        font_scale: 1.0,
        windows: Vec::new(),
        zoom_labels: Vec::new(),
    });
}

pub fn current_font_scale() -> f64 {
    STATE.with(|s| s.borrow().font_scale)
}

pub fn register_zoom_label(btn: gtk::Button) {
    STATE.with(|s| {
        let wr = glib::WeakRef::new();
        wr.set(Some(&btn));
        s.borrow_mut().zoom_labels.push(wr);
    });
}

pub fn register_window(w: &Rc<OrbitWindow>) {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.windows.retain(|w| w.upgrade().is_some());
        state.windows.push(Rc::downgrade(w));
    });
}

fn set_font_scale(scale: f64) {
    let clamped = scale.clamp(0.5, 3.0);
    let windows: Vec<Rc<OrbitWindow>> = STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.font_scale = clamped;
        state
            .windows
            .iter()
            .filter_map(|w| w.upgrade())
            .collect()
    });
    for w in windows {
        w.apply_font_scale(clamped);
    }
    let pct = format!("{}%", (clamped * 100.0).round() as i32);
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.zoom_labels.retain(|w| w.upgrade().is_some());
        for w in &state.zoom_labels {
            if let Some(btn) = w.upgrade() {
                btn.set_label(&pct);
            }
        }
    });
}

pub struct OrbitApp;

impl OrbitApp {
    pub fn run() -> glib::ExitCode {
        let application = adw::Application::builder()
            .application_id(APP_ID)
            .flags(gio::ApplicationFlags::FLAGS_NONE)
            .build();

        application.connect_startup(|_| {
            load_css();
        });

        application.connect_activate(|app| {
            let window = OrbitWindow::new(app);
            register_window(&window);
            window.present();
            // Keep the Rc alive; Weak refs in STATE upgrade for the app's lifetime.
            std::mem::forget(window);
        });

        register_actions(&application);
        application.run()
    }
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("../data/style.css"));

    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn register_actions(app: &adw::Application) {
    // Window-scoped accelerators
    app.set_accels_for_action("win.new-workspace", &["<Primary>t"]);
    app.set_accels_for_action("win.close-workspace", &["<Primary>w"]);
    app.set_accels_for_action("win.new-session", &["<Primary><Shift>Return"]);
    app.set_accels_for_action("win.toggle-split", &["<Primary><Shift>e"]);
    app.set_accels_for_action("win.tab-overview", &["<Primary><Shift>o"]);
    app.set_accels_for_action("win.fullscreen", &["<Primary><Shift>F11"]);
    for i in 1..=9 {
        app.set_accels_for_action(
            &format!("win.focus-session({i})"),
            &[&format!("<Super>{i}")],
        );
        app.set_accels_for_action(
            &format!("win.toggle-session({i})"),
            &[&format!("<Super><Shift>{i}")],
        );
    }

    // App-scoped accelerators
    app.set_accels_for_action("app.new-window", &["<Primary><Shift>n"]);
    app.set_accels_for_action("app.zoom-in", &["<Primary>plus", "<Primary>equal"]);
    app.set_accels_for_action("app.zoom-out", &["<Primary>minus"]);
    app.set_accels_for_action("app.zoom-reset", &["<Primary>0"]);
    app.set_accels_for_action("app.shortcuts", &["<Primary>question"]);
    app.set_accels_for_action("app.quit", &["<Primary>q"]);

    // App-level actions
    let quit = gio::ActionEntry::builder("quit")
        .activate(|a: &adw::Application, _, _| a.quit())
        .build();

    let new_window = gio::ActionEntry::builder("new-window")
        .activate(|a: &adw::Application, _, _| {
            let win = OrbitWindow::new(a);
            register_window(&win);
            win.present();
            std::mem::forget(win);
        })
        .build();

    let preferences = gio::ActionEntry::builder("preferences")
        .activate(|a: &adw::Application, _, _| {
            let parent = a.active_window();
            crate::prefs::show(parent.as_ref());
        })
        .build();

    let shortcuts = gio::ActionEntry::builder("shortcuts")
        .activate(|a: &adw::Application, _, _| {
            let parent = a.active_window();
            crate::shortcuts::show(parent.as_ref());
        })
        .build();

    let about = gio::ActionEntry::builder("about")
        .activate(|a: &adw::Application, _, _| {
            let parent = a.active_window();
            crate::about::show(parent.as_ref());
        })
        .build();

    let zoom_in = gio::ActionEntry::builder("zoom-in")
        .activate(|_a: &adw::Application, _, _| {
            set_font_scale(current_font_scale() + 0.1);
        })
        .build();

    let zoom_out = gio::ActionEntry::builder("zoom-out")
        .activate(|_a: &adw::Application, _, _| {
            set_font_scale(current_font_scale() - 0.1);
        })
        .build();

    let zoom_reset = gio::ActionEntry::builder("zoom-reset")
        .activate(|_a: &adw::Application, _, _| {
            set_font_scale(1.0);
        })
        .build();

    app.add_action_entries([
        quit,
        new_window,
        preferences,
        shortcuts,
        about,
        zoom_in,
        zoom_out,
        zoom_reset,
    ]);
}
