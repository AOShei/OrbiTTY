use adw::prelude::*;
use gtk::gio;
use gtk4 as gtk;
use libadwaita as adw;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::menu;
use crate::templates::WorkspaceTemplate;
use crate::workspace::Workspace;

pub struct OrbitWindow {
    pub window: adw::ApplicationWindow,
    tab_view: adw::TabView,
    tab_overview: adw::TabOverview,
    workspaces: Rc<RefCell<HashMap<u32, Workspace>>>,
    next_ws_id: Rc<RefCell<u32>>,
    page_map: Rc<RefCell<HashMap<String, u32>>>,
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

        // Main app menu on the far right; new-workspace button sits to its left.
        let menu_btn = menu::build_main_menu_button();
        header.pack_end(&menu_btn);

        let new_btn = gtk::Button::from_icon_name("tab-new-symbolic");
        new_btn.set_tooltip_text(Some("New Workspace"));
        new_btn.set_action_name(Some("win.new-workspace"));
        header.pack_end(&new_btn);

        // Toolbar layout: header + tab-bar on top, tab-view as content.
        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.add_top_bar(&tab_bar);
        toolbar_view.set_content(Some(&tab_view));

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
        });

        this.install_actions();

        // The "+" inside the overview creates a new empty workspace.
        {
            let weak = Rc::downgrade(&this);
            tab_overview.connect_create_tab(move |_| {
                let this = weak.upgrade().expect("window dropped");
                let t = WorkspaceTemplate::by_name("Empty")
                    .expect("Empty template must exist");
                this.open_workspace(&t)
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

        if let Some(default_t) = WorkspaceTemplate::by_name("Empty") {
            this.open_workspace(&default_t);
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

    fn page_key(&self, page: &adw::TabPage) -> String {
        format!("{:p}", page.as_ptr())
    }

    fn open_workspace(&self, template: &WorkspaceTemplate) -> adw::TabPage {
        let id = {
            let mut n = self.next_ws_id.borrow_mut();
            let id = *n;
            *n += 1;
            id
        };

        let name = template.name.to_string();
        let ws = Workspace::new(id, &name, template);
        let page = self.tab_view.append(&ws.widget());
        page.set_title(&name);

        let key = self.page_key(&page);
        self.workspaces.borrow_mut().insert(id, ws.clone());
        self.page_map.borrow_mut().insert(key, id);

        self.renumber_duplicates(template.name);
        self.tab_view.set_selected_page(&page);
        page
    }

    fn renumber_duplicates(&self, template_name: &str) {
        let mut same: Vec<adw::TabPage> = Vec::new();
        let n = self.tab_view.n_pages();
        for i in 0..n {
            let page = self.tab_view.nth_page(i);
            let key = self.page_key(&page);
            if let Some(&wid) = self.page_map.borrow().get(&key) {
                if let Some(ws) = self.workspaces.borrow().get(&wid) {
                    if ws.inner.borrow().template_name == template_name {
                        same.push(page);
                    }
                }
            }
        }

        if same.len() <= 1 {
            if let Some(p) = same.first() {
                p.set_title(template_name);
            }
            return;
        }
        for (i, page) in same.iter().enumerate() {
            page.set_title(&format!("{} ({})", template_name, i + 1));
        }
    }

    fn current_workspace(&self) -> Option<Workspace> {
        let page = self.tab_view.selected_page()?;
        let key = self.page_key(&page);
        let id = self.page_map.borrow().get(&key).copied()?;
        self.workspaces.borrow().get(&id).cloned()
    }

    fn install_actions(self: &Rc<Self>) {
        let group = gio::SimpleActionGroup::new();

        // Ctrl+T: new empty workspace
        {
            let weak = Rc::downgrade(self);
            let a = gio::SimpleAction::new("new-workspace", None);
            a.connect_activate(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    if let Some(t) = WorkspaceTemplate::by_name("Empty") {
                        this.open_workspace(&t);
                    }
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
        {
            let weak = Rc::downgrade(self);
            let a = gio::SimpleAction::new("toggle-session", Some(&i32::static_variant_type()));
            a.connect_activate(move |_, param| {
                if let Some(this) = weak.upgrade() {
                    if let Some(v) = param {
                        if let Some(i) = v.get::<i32>() {
                            if let Some(ws) = this.current_workspace() {
                                ws.toggle_index(i as usize);
                            }
                        }
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

        self.window.insert_action_group("win", Some(&group));
    }
}
