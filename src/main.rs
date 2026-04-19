#![allow(dead_code)]

mod about;
mod app;
mod arena;
mod menu;
mod prefs;
mod session;
mod shortcuts;
mod sidebar;
mod templates;
mod window;
mod workspace;

use app::OrbitApp;

fn main() -> glib::ExitCode {
    OrbitApp::run()
}
