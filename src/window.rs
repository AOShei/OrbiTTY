use adw::prelude::*;
use gtk::gio;
use gtk4 as gtk;
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::filetree::{self, FileTree};
use crate::menu;
use crate::templates::WorkspaceTemplate;
use crate::workspace::Workspace;

/// Show a lightweight name-entry popup. Enter confirms, Escape/click-outside cancels.
fn show_name_dialog(
    parent: &adw::ApplicationWindow,
    heading: &str,
    initial_text: &str,
    on_confirm: impl FnOnce(String) + 'static,
) {
    let header = adw::HeaderBar::builder()
        .show_end_title_buttons(false)
        .show_start_title_buttons(false)
        .decoration_layout(":")
        .build();
    let title = gtk::Label::new(Some(heading));
    title.add_css_class("heading");
    header.set_title_widget(Some(&title));

    let entry = gtk::Entry::builder()
        .placeholder_text("Workspace name")
        .margin_start(12)
        .margin_end(12)
        .margin_top(8)
        .margin_bottom(12)
        .build();
    entry.set_text(initial_text);

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.append(&entry);

    let dialog = gtk::Window::builder()
        .transient_for(parent)
        .modal(false)
        .default_width(300)
        .resizable(false)
        .titlebar(&header)
        .child(&vbox)
        .build();

    let cb: Rc<RefCell<Option<Box<dyn FnOnce(String)>>>> =
        Rc::new(RefCell::new(Some(Box::new(on_confirm))));
    let closed: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    let do_close = {
        let closed = closed.clone();
        let d = dialog.clone();
        move || {
            if !closed.get() {
                closed.set(true);
                d.destroy();
            }
        }
    };

    // Enter confirms and closes.
    let cb2 = cb.clone();
    let close = do_close.clone();
    entry.connect_activate(move |entry| {
        let name = entry.text().trim().to_string();
        close();
        if !name.is_empty() {
            if let Some(f) = cb2.borrow_mut().take() {
                f(name);
            }
        }
    });

    // Escape key closes the dialog.
    let close = do_close.clone();
    let esc = gtk::EventControllerKey::new();
    esc.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            close();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    dialog.add_controller(esc);

    // Click outside: window loses active → close.
    let close = do_close.clone();
    dialog.connect_is_active_notify(move |win| {
        if !win.is_active() {
            close();
        }
    });

    dialog.present();
    entry.grab_focus();
    if !initial_text.is_empty() {
        entry.select_region(0, -1);
    }
}

pub struct OrbitWindow {
    pub window: adw::ApplicationWindow,
    tab_view: adw::TabView,
    tab_overview: adw::TabOverview,
    workspaces: Rc<RefCell<HashMap<u32, Workspace>>>,
    next_ws_id: Rc<RefCell<u32>>,
    page_map: Rc<RefCell<HashMap<String, u32>>>,
    file_tree: FileTree,
    content_paned: gtk::Paned,
    filetree_saved_width: Rc<Cell<i32>>,
}

impl OrbitWindow {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("OrbiTTY")
            .default_width(1400)
            .default_height(900)
            .build();

        let tab_view = adw::TabView::new();
        tab_view.set_vexpand(true);
        tab_view.set_hexpand(true);

        let tab_bar = adw::TabBar::new();
        tab_bar.set_view(Some(&tab_view));
        tab_bar.set_autohide(true);
        tab_bar.set_expand_tabs(true);

        // --- Header ---
        let header = adw::HeaderBar::new();
        let title = adw::WindowTitle::new("OrbiTTY", "Mission Control for Terminals");
        header.set_title_widget(Some(&title));

        // Tab overview toggle on the left.
        let overview_btn = adw::TabButton::new();
        overview_btn.set_view(Some(&tab_view));
        overview_btn.set_action_name(Some("win.tab-overview"));
        overview_btn.set_tooltip_text(Some("Show All Tabs"));
        header.pack_start(&overview_btn);

        // File tree toggle button in the header
        let filetree_btn = gtk::Button::from_icon_name("folder-symbolic");
        filetree_btn.set_tooltip_text(Some("Toggle File Tree (Alt+E)"));
        filetree_btn.set_action_name(Some("win.toggle-filetree"));
        header.pack_start(&filetree_btn);

        // Main app menu on the far right; new-workspace button sits to its left.
        let menu_btn = menu::build_main_menu_button();
        header.pack_end(&menu_btn);

        let new_ws_btn = gtk::Button::from_icon_name("tab-new-symbolic");
        new_ws_btn.set_tooltip_text(Some("New Workspace"));
        new_ws_btn.set_action_name(Some("win.new-workspace"));
        header.pack_end(&new_ws_btn);

        let new_term_btn = gtk::Button::from_icon_name("list-add-symbolic");
        new_term_btn.set_tooltip_text(Some("New Terminal"));
        new_term_btn.set_action_name(Some("win.new-session"));
        header.pack_end(&new_term_btn);

        // --- File tree dock (left side) ---
        let file_tree = FileTree::new();
        let initially_open = filetree::load_open_state();

        // Left-dock paned: file tree | tab view.
        let content_paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .resize_start_child(false)
            .shrink_start_child(true)
            .resize_end_child(true)
            .shrink_end_child(false)
            .build();
        content_paned.set_start_child(Some(&file_tree.root));
        content_paned.set_end_child(Some(&tab_view));

        if initially_open {
            file_tree.root.set_visible(true);
            content_paned.set_position(240);
        } else {
            file_tree.root.set_visible(false);
        }

        // Toolbar layout: header + tab-bar on top, paned as content.
        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.add_top_bar(&tab_bar);
        toolbar_view.set_content(Some(&content_paned));

        // Wrap in TabOverview so Ctrl+Shift+O opens the zoomed-out grid.
        let tab_overview = adw::TabOverview::new();
        tab_overview.set_view(Some(&tab_view));
        tab_overview.set_enable_new_tab(true);
        tab_overview.set_child(Some(&toolbar_view));

        window.set_content(Some(&tab_overview));

        let this = Rc::new(OrbitWindow {
            window: window.clone(),
            tab_view: tab_view.clone(),
            tab_overview: tab_overview.clone(),
            workspaces: Rc::new(RefCell::new(HashMap::new())),
            next_ws_id: Rc::new(RefCell::new(1)),
            page_map: Rc::new(RefCell::new(HashMap::new())),
            file_tree,
            content_paned,
            filetree_saved_width: Rc::new(Cell::new(240)),
        });

        this.install_actions();

        // Wire the file tree "open terminal here" callback now that we have `this`.
        {
            let weak = Rc::downgrade(&this);
            *this.file_tree.open_cb.borrow_mut() = Box::new(move |path: String| {
                if let Some(w) = weak.upgrade() {
                    w.open_terminal_at(path);
                }
            });
        }

        // Wire the file tree "cd here" callback (double-click → cd focused terminal).
        {
            let weak = Rc::downgrade(&this);
            *this.file_tree.cd_cb.borrow_mut() = Box::new(move |path: String| -> bool {
                let Some(this) = weak.upgrade() else { return false };
                let Some(ws) = this.current_workspace() else { return false };
                let Some(session) = ws.focused_session() else { return false };
                if session.is_busy() {
                    return false;
                }
                session.send_cd(&path);
                true
            });
        }

        // The "+" inside the overview creates a workspace and prompts for a name.
        {
            let weak = Rc::downgrade(&this);
            tab_overview.connect_create_tab(move |_| {
                let this = weak.upgrade().expect("window dropped");
                let t = WorkspaceTemplate::by_name("Empty")
                    .expect("Empty template must exist");
                let page = this.open_workspace(&t, "Untitled");
                let weak2 = Rc::downgrade(&this);
                let page_clone = page.clone();
                glib::idle_add_local_once(move || {
                    if let Some(this) = weak2.upgrade() {
                        this.rename_page(&page_clone);
                    }
                });
                page
            });
        }

        {
            let weak = Rc::downgrade(&this);
            tab_view.connect_close_page(move |view, page| {
                if view.n_pages() <= 1 {
                    view.close_page_finish(page, false);
                    return glib::Propagation::Stop;
                }
                if let Some(this) = weak.upgrade() {
                    let key = this.page_key(page);
                    if let Some(id) = this.page_map.borrow_mut().remove(&key) {
                        this.workspaces.borrow_mut().remove(&id);
                    }
                }
                view.close_page_finish(page, true);
                glib::Propagation::Stop
            });
        }

        // When switching tabs, focus the first terminal in the new workspace.
        {
            let weak = Rc::downgrade(&this);
            tab_view.connect_selected_page_notify(move |_| {
                let weak2 = weak.clone();
                glib::idle_add_local_once(move || {
                    let Some(this) = weak2.upgrade() else { return };
                    if let Some(ws) = this.current_workspace() {
                        ws.refocus();
                    }
                });
            });
        }

        if let Some(default_t) = WorkspaceTemplate::by_name("Empty") {
            this.open_workspace(&default_t, "Workspace");
        }

        this
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn apply_font_scale(&self, scale: f64) {
        for ws in self.workspaces.borrow().values() {
            for s in ws.sessions() {
                s.set_font_scale(scale);
            }
        }
    }

    /// Open a new terminal session at the given directory path.
    fn open_terminal_at(&self, path: String) {
        let Some(ws) = self.current_workspace() else { return };
        let name = std::path::Path::new(&path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        // promote=true: tries arena first, falls back to sidebar if arena is full (≥4).
        let session = ws.spawn_session(&name, Some(&path), true);
        session.grab_focus();
    }

    fn page_key(&self, page: &adw::TabPage) -> String {
        format!("{:p}", page.as_ptr())
    }

    fn open_workspace(&self, template: &WorkspaceTemplate, name: &str) -> adw::TabPage {
        let id = {
            let mut n = self.next_ws_id.borrow_mut();
            let id = *n;
            *n += 1;
            id
        };

        let ws = Workspace::new(template);

        // Wire CWD updates from this workspace to the file tree.
        let ft = self.file_tree.clone();
        ws.set_cwd_change_cb(move |cwd: Option<String>| {
            if let Some(path) = cwd {
                ft.set_cwd(&path);
            }
        });

        let page = self.tab_view.append(&ws.widget());
        page.set_title(name);

        let key = self.page_key(&page);
        self.workspaces.borrow_mut().insert(id, ws.clone());
        self.page_map.borrow_mut().insert(key, id);

        self.tab_view.set_selected_page(&page);

        // Focus the first terminal so the user can start typing immediately.
        ws.refocus();

        page
    }

    fn rename_page(&self, page: &adw::TabPage) {
        let current_title = page.title().to_string();
        let page = page.clone();

        show_name_dialog(
            &self.window,
            "Rename Workspace",
            &current_title,
            move |name| {
                page.set_title(&name);
            },
        );
    }

    fn current_workspace(&self) -> Option<Workspace> {
        let page = self.tab_view.selected_page()?;
        let key = self.page_key(&page);
        let id = self.page_map.borrow().get(&key).copied()?;
        self.workspaces.borrow().get(&id).cloned()
    }

    fn install_actions(self: &Rc<Self>) {
        let group = gio::SimpleActionGroup::new();

        // Ctrl+T: new empty workspace (prompt for name first)
        {
            let weak = Rc::downgrade(self);
            let a = gio::SimpleAction::new("new-workspace", None);
            a.connect_activate(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    let weak2 = Rc::downgrade(&this);
                    show_name_dialog(
                        &this.window,
                        "New Workspace",
                        "",
                        move |name| {
                            if let Some(this) = weak2.upgrade() {
                                if let Some(t) = WorkspaceTemplate::by_name("Empty") {
                                    this.open_workspace(&t, &name);
                                }
                            }
                        },
                    );
                }
            });
            group.add_action(&a);
        }

        // Ctrl+W
        {
            let weak = Rc::downgrade(self);
            let a = gio::SimpleAction::new("close-workspace", None);
            a.connect_activate(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    if this.tab_view.n_pages() <= 1 {
                        return;
                    }
                    if let Some(page) = this.tab_view.selected_page() {
                        this.tab_view.close_page(&page);
                    }
                }
            });
            group.add_action(&a);
        }

        // Ctrl+Shift+Return: new shell in current workspace
        {
            let weak = Rc::downgrade(self);
            let a = gio::SimpleAction::new("new-session", None);
            a.connect_activate(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    if let Some(ws) = this.current_workspace() {
                        ws.new_session();
                    }
                }
            });
            group.add_action(&a);
        }

        // Ctrl+Shift+E
        {
            let weak = Rc::downgrade(self);
            let a = gio::SimpleAction::new("toggle-split", None);
            a.connect_activate(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    if let Some(ws) = this.current_workspace() {
                        ws.toggle_split();
                    }
                }
            });
            group.add_action(&a);
        }

        // Super+1..9 / Super+Shift+1..9
        {
            let weak = Rc::downgrade(self);
            let a = gio::SimpleAction::new("focus-session", Some(&i32::static_variant_type()));
            a.connect_activate(move |_, param| {
                if let Some(this) = weak.upgrade() {
                    if let Some(v) = param {
                        if let Some(i) = v.get::<i32>() {
                            if let Some(ws) = this.current_workspace() {
                                ws.focus_index(i as usize);
                            }
                        }
                    }
                }
            });
            group.add_action(&a);
        }

        // Ctrl+Shift+D: demote the currently focused arena session to the sidebar.
        {
            let weak = Rc::downgrade(self);
            let a = gio::SimpleAction::new("demote-focused", None);
            a.connect_activate(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    if let Some(ws) = this.current_workspace() {
                        ws.demote_focused();
                    }
                }
            });
            group.add_action(&a);
        }

        // Ctrl+Shift+F: make the focused session the sole occupant of the arena.
        {
            let weak = Rc::downgrade(self);
            let a = gio::SimpleAction::new("solo-session", None);
            a.connect_activate(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    if let Some(ws) = this.current_workspace() {
                        ws.solo_focused();
                    }
                }
            });
            group.add_action(&a);
        }

        // Alt+Space: peek the most-relevant sidebar card (attention first, else first).
        {
            let weak = Rc::downgrade(self);
            let a = gio::SimpleAction::new("peek-sidebar", None);
            a.connect_activate(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    if let Some(ws) = this.current_workspace() {
                        ws.peek_best();
                    }
                }
            });
            group.add_action(&a);
        }

        // Ctrl+Tab / Ctrl+Shift+Tab: cycle focus through arena sessions
        {
            let weak = Rc::downgrade(self);
            let a = gio::SimpleAction::new("cycle-session-next", None);
            a.connect_activate(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    if let Some(ws) = this.current_workspace() {
                        ws.focus_next();
                    }
                }
            });
            group.add_action(&a);
        }
        {
            let weak = Rc::downgrade(self);
            let a = gio::SimpleAction::new("cycle-session-prev", None);
            a.connect_activate(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    if let Some(ws) = this.current_workspace() {
                        ws.focus_prev();
                    }
                }
            });
            group.add_action(&a);
        }

        // Ctrl+Shift+O: toggle tab overview
        {
            let weak = Rc::downgrade(self);
            let a = gio::SimpleAction::new("tab-overview", None);
            a.connect_activate(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    let open = this.tab_overview.is_open();
                    this.tab_overview.set_open(!open);
                }
            });
            group.add_action(&a);
        }

        // F2: rename current workspace
        {
            let weak = Rc::downgrade(self);
            let a = gio::SimpleAction::new("rename-workspace", None);
            a.connect_activate(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    if let Some(page) = this.tab_view.selected_page() {
                        this.rename_page(&page);
                    }
                }
            });
            group.add_action(&a);
        }

        // Ctrl+Shift+F11: toggle fullscreen
        {
            let weak = Rc::downgrade(self);
            let a = gio::SimpleAction::new("fullscreen", None);
            a.connect_activate(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    if this.window.is_fullscreen() {
                        this.window.unfullscreen();
                    } else {
                        this.window.fullscreen();
                    }
                }
            });
            group.add_action(&a);
        }

        // Alt+E: toggle the file tree dock
        {
            let weak = Rc::downgrade(self);
            let a = gio::SimpleAction::new("toggle-filetree", None);
            a.connect_activate(move |_, _| {
                let Some(this) = weak.upgrade() else { return };
                let currently_open = this.file_tree.root.is_visible();
                if currently_open {
                    this.filetree_saved_width.set(this.content_paned.position());
                    this.file_tree.root.set_visible(false);
                    filetree::save_open_state(false);
                } else {
                    this.file_tree.root.set_visible(true);
                    let saved = this.filetree_saved_width.get();
                    let pos = if saved > 0 { saved } else { 240 };
                    let paned = this.content_paned.clone();
                    glib::idle_add_local_once(move || {
                        paned.set_position(pos);
                    });
                    filetree::save_open_state(true);
                }
            });
            group.add_action(&a);
        }

        self.window.insert_action_group("win", Some(&group));
    }
}
