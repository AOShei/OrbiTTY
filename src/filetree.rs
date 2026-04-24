use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const MAX_CHILDREN: usize = 200;

/// Alt+click callback: open a new terminal at the given path.
pub type OpenTerminalCb = Rc<RefCell<Box<dyn Fn(String)>>>;
/// Double-click callback: cd the focused terminal to the given path.
/// Returns true if the cd was sent, false if the terminal was busy.
pub type CdTerminalCb = Rc<RefCell<Box<dyn Fn(String) -> bool>>>;

#[derive(Clone)]
struct NodeData {
    row_box: gtk::Box,
    children_box: gtk::Box,
    arrow: gtk::Button,
    is_expanded: Rc<Cell<bool>>,
    children_loaded: Rc<Cell<bool>>,
}

#[derive(Clone)]
pub struct FileTree {
    pub root: gtk::Box,
    pub open_cb: OpenTerminalCb,
    pub cd_cb: CdTerminalCb,
    tree_box: gtk::Box,
    scroller: gtk::ScrolledWindow,
    nodes: Rc<RefCell<HashMap<PathBuf, NodeData>>>,
    cwd: Rc<RefCell<Option<PathBuf>>>,
    show_hidden: Rc<Cell<bool>>,
}

impl FileTree {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("orbit-filetree");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        header.add_css_class("orbit-filetree-header");

        let title = gtk::Label::new(Some("Explorer"));
        title.add_css_class("heading");
        title.set_halign(gtk::Align::Start);
        title.set_hexpand(true);
        header.append(&title);

        let initially_show_hidden = load_show_hidden_state();
        let hidden_btn = gtk::ToggleButton::new();
        hidden_btn.add_css_class("flat");
        hidden_btn.set_focusable(false);
        if initially_show_hidden {
            hidden_btn.set_active(true);
            hidden_btn.set_icon_name("view-conceal-symbolic");
            hidden_btn.set_tooltip_text(Some("Hide Hidden Directories"));
        } else {
            hidden_btn.set_icon_name("view-reveal-symbolic");
            hidden_btn.set_tooltip_text(Some("Show Hidden Directories"));
        }
        header.append(&hidden_btn);

        let tree_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        tree_box.set_valign(gtk::Align::Start);
        tree_box.set_hexpand(true);

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .hexpand(true)
            .focusable(false)
            .build();
        scroller.set_child(Some(&tree_box));

        root.append(&header);
        root.append(&scroller);

        let open_cb: OpenTerminalCb = Rc::new(RefCell::new(Box::new(|_: String| {})));
        let cd_cb: CdTerminalCb = Rc::new(RefCell::new(Box::new(|_: String| true)));

        let ft = FileTree {
            root,
            open_cb,
            cd_cb,
            tree_box,
            scroller,
            nodes: Rc::new(RefCell::new(HashMap::new())),
            cwd: Rc::new(RefCell::new(None)),
            show_hidden: Rc::new(Cell::new(initially_show_hidden)),
        };

        {
            let ft = ft.clone();
            hidden_btn.connect_toggled(move |btn| {
                if btn.is_active() {
                    btn.set_icon_name("view-conceal-symbolic");
                    btn.set_tooltip_text(Some("Hide Hidden Directories"));
                } else {
                    btn.set_icon_name("view-reveal-symbolic");
                    btn.set_tooltip_text(Some("Show Hidden Directories"));
                }
                ft.show_hidden.set(btn.is_active());
                save_show_hidden_state(btn.is_active());
                ft.rebuild();
            });
        }

        ft.add_node(&PathBuf::from("/"), &ft.tree_box);
        ft
    }

    /// Update the highlighted CWD and expand all ancestor directories.
    pub fn set_cwd(&self, cwd_str: &str) {
        let new_path = PathBuf::from(cwd_str);

        // Un-highlight previous CWD
        {
            let old = self.cwd.borrow().clone();
            if let Some(old_path) = old {
                if let Some(n) = self.nodes.borrow().get(&old_path) {
                    n.row_box.remove_css_class("orbit-cwd");
                }
            }
        }

        // Build ancestor chain from / down to new_path
        let mut ancestors: Vec<PathBuf> = new_path
            .ancestors()
            .map(|p| p.to_path_buf())
            .collect();
        ancestors.reverse(); // now root-first

        // Expand each ancestor so the next level exists in the tree
        for ancestor in &ancestors {
            self.ensure_expanded(ancestor);
        }

        // Highlight the new CWD row
        if let Some(n) = self.nodes.borrow().get(&new_path) {
            n.row_box.add_css_class("orbit-cwd");
        }

        // Scroll to the CWD row after GTK finishes the layout pass
        let this = self.clone();
        let path = new_path.clone();
        glib::idle_add_local_once(move || {
            this.scroll_to(&path);
        });

        *self.cwd.borrow_mut() = Some(new_path);
    }

    fn ensure_expanded(&self, path: &PathBuf) {
        let should = {
            let nodes = self.nodes.borrow();
            match nodes.get(path) {
                Some(n) => !n.is_expanded.get(),
                None => false,
            }
        };
        if should {
            self.expand(path);
        }
    }

    fn expand(&self, path: &PathBuf) {
        let (children_loaded, children_box, arrow, is_expanded_flag) = {
            let nodes = self.nodes.borrow();
            let Some(n) = nodes.get(path) else { return };
            if n.is_expanded.get() {
                return;
            }
            (
                n.children_loaded.clone(),
                n.children_box.clone(),
                n.arrow.clone(),
                n.is_expanded.clone(),
            )
        };

        if !children_loaded.get() {
            self.load_children(path, &children_box);
            children_loaded.set(true);
        }

        children_box.set_visible(true);
        arrow.set_icon_name("pan-down-symbolic");
        is_expanded_flag.set(true);
    }

    fn collapse(&self, path: &PathBuf) {
        let (children_box, arrow, is_expanded_flag) = {
            let nodes = self.nodes.borrow();
            let Some(n) = nodes.get(path) else { return };
            (n.children_box.clone(), n.arrow.clone(), n.is_expanded.clone())
        };
        children_box.set_visible(false);
        arrow.set_icon_name("pan-end-symbolic");
        is_expanded_flag.set(false);
    }

    fn load_children(&self, path: &PathBuf, children_box: &gtk::Box) {
        let show_hidden = self.show_hidden.get();
        let Ok(rd) = std::fs::read_dir(path) else {
            return;
        };
        let mut dirs: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter(|e| show_hidden || !e.file_name().to_string_lossy().starts_with('.'))
            .map(|e| e.path())
            .collect();
        dirs.sort();
        for child in dirs.into_iter().take(MAX_CHILDREN) {
            self.add_node(&child, children_box);
        }
    }

    fn add_node(&self, path: &PathBuf, parent_box: &gtk::Box) {
        let depth = path_depth(path);
        let name = if path == Path::new("/") {
            "/".to_string()
        } else {
            path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        };

        // A directory is locked if we can't read its contents (permission denied).
        let locked = path != Path::new("/") && std::fs::read_dir(path).is_err();

        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        container.set_hexpand(true);

        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row_box.add_css_class("orbit-filetree-row");
        row_box.set_hexpand(true);
        row_box.set_margin_start(depth as i32 * 16);

        let arrow = gtk::Button::new();
        arrow.set_icon_name("pan-end-symbolic");
        arrow.add_css_class("flat");
        arrow.add_css_class("orbit-filetree-arrow");
        arrow.set_focusable(false);

        let name_btn = gtk::Button::new();
        name_btn.add_css_class("flat");
        name_btn.add_css_class("orbit-filetree-name");
        name_btn.set_hexpand(true);
        name_btn.set_halign(gtk::Align::Fill);
        name_btn.set_focusable(false);
        let name_label = gtk::Label::new(Some(&name));
        name_label.set_xalign(0.0);
        name_label.set_hexpand(true);
        name_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name_btn.set_child(Some(&name_label));

        row_box.append(&arrow);
        row_box.append(&name_btn);

        if locked {
            // Keep arrow in layout so name labels align with expandable dirs.
            arrow.set_opacity(0.0);
            arrow.set_sensitive(false);
            let lock_icon = gtk::Image::from_icon_name("changes-prevent-symbolic");
            lock_icon.add_css_class("orbit-filetree-lock");
            row_box.append(&lock_icon);
        }

        let children_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        children_box.set_visible(false);

        container.append(&row_box);
        container.append(&children_box);
        parent_box.append(&container);

        let is_expanded = Rc::new(Cell::new(false));
        let children_loaded = Rc::new(Cell::new(false));

        // Arrow toggles expand/collapse
        {
            let path = path.clone();
            let ft = self.clone();
            arrow.connect_clicked(move |_| {
                let expanded = ft
                    .nodes
                    .borrow()
                    .get(&path)
                    .map(|n| n.is_expanded.get())
                    .unwrap_or(false);
                if expanded {
                    ft.collapse(&path);
                } else {
                    ft.expand(&path);
                }
            });
        }

        // Name button: double-click → cd focused terminal; Alt+click → open new terminal
        {
            let path_str = path.to_string_lossy().to_string();
            let open_cb = self.open_cb.clone();
            let cd_cb = self.cd_cb.clone();
            let row_for_flash = row_box.clone();
            let gesture = gtk::GestureClick::new();
            gesture.set_button(gtk::gdk::BUTTON_PRIMARY);
            gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
            gesture.connect_pressed(move |g, n_press, _, _| {
                // Claim the event so the button doesn't try to handle it (and potentially take focus).
                g.set_state(gtk::EventSequenceState::Claimed);
                let mods = g.current_event_state();
                if mods.contains(gtk::gdk::ModifierType::ALT_MASK) {
                    (open_cb.borrow())(path_str.clone());
                } else if n_press == 2 {
                    let sent = (cd_cb.borrow())(path_str.clone());
                    if !sent {
                        let row = row_for_flash.clone();
                        row.add_css_class("orbit-busy-flash");
                        glib::timeout_add_local_once(
                            std::time::Duration::from_millis(500),
                            move || { row.remove_css_class("orbit-busy-flash"); },
                        );
                    }
                }
            });
            name_btn.add_controller(gesture);
        }

        self.nodes.borrow_mut().insert(
            path.clone(),
            NodeData {
                row_box,
                children_box,
                arrow,
                is_expanded,
                children_loaded,
            },
        );
    }

    /// Tear down and rebuild the tree from scratch, then re-expand to the
    /// current CWD. Called when the show-hidden toggle changes.
    pub fn rebuild(&self) {
        while let Some(child) = self.tree_box.first_child() {
            self.tree_box.remove(&child);
        }
        self.nodes.borrow_mut().clear();
        self.add_node(&PathBuf::from("/"), &self.tree_box);
        let cwd = self.cwd.borrow().clone();
        *self.cwd.borrow_mut() = None;
        if let Some(path) = cwd {
            if let Some(s) = path.to_str() {
                self.set_cwd(s);
            }
        }
    }

    fn scroll_to(&self, path: &PathBuf) {
        let row = {
            let nodes = self.nodes.borrow();
            nodes.get(path).map(|n| n.row_box.clone())
        };
        let Some(row) = row else { return };
        let Some(p) = row.compute_point(&self.tree_box, &gtk::graphene::Point::new(0.0, 0.0))
        else {
            return;
        };

        let adj = self.scroller.vadjustment();
        let y = p.y() as f64;
        let h = row.height().max(1) as f64;
        let view_top = adj.value();
        let view_bot = view_top + adj.page_size();

        if y < view_top {
            adj.set_value((y - 8.0).max(0.0));
        } else if y + h > view_bot {
            adj.set_value((y + h - adj.page_size() + 8.0).max(0.0));
        }
    }
}

fn path_depth(path: &Path) -> u32 {
    (path.components().count() as u32).saturating_sub(1)
}

// --- Persistent open/closed state ---

fn state_file_path() -> PathBuf {
    let state_home = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(".local").join("state")
        });
    state_home.join("orbitty").join("filetree_open")
}

pub fn load_open_state() -> bool {
    state_file_path().exists()
}

pub fn save_open_state(open: bool) {
    let path = state_file_path();
    if open {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, b"");
    } else {
        let _ = std::fs::remove_file(&path);
    }
}

// --- Persistent show-hidden state ---

fn show_hidden_state_file_path() -> PathBuf {
    let state_home = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(".local").join("state")
        });
    state_home.join("orbitty").join("filetree_show_hidden")
}

pub fn load_show_hidden_state() -> bool {
    show_hidden_state_file_path().exists()
}

pub fn save_show_hidden_state(show: bool) {
    let path = show_hidden_state_file_path();
    if show {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, b"");
    } else {
        let _ = std::fs::remove_file(&path);
    }
}
