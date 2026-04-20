use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::RefCell;
use std::rc::Rc;

use crate::session::Session;

pub const MAX_ACTIVE: usize = 4;

/// Tiling Arena: manages up to 4 VTE sessions arranged in a GtkGrid.
#[derive(Clone)]
pub struct Arena {
    pub grid: gtk::Grid,
    pub sessions: Rc<RefCell<Vec<Session>>>,
    pub empty_state: gtk::Box,
    pub split_horizontal: Rc<RefCell<bool>>,
    /// Placeholder widget shown during drag preview.
    phantom: Rc<RefCell<Option<gtk::Box>>>,
    /// Saved session order before a preview swap; restored on drag leave.
    saved_order: Rc<RefCell<Option<Vec<u32>>>>,
}

impl Arena {
    pub fn new() -> Self {
        let grid = gtk::Grid::builder()
            .row_homogeneous(true)
            .column_homogeneous(true)
            .row_spacing(0)
            .column_spacing(0)
            .hexpand(true)
            .vexpand(true)
            .build();
        grid.add_css_class("orbit-arena");

        let empty_state = build_empty_state();
        grid.attach(&empty_state, 0, 0, 2, 2);

        Arena {
            grid,
            sessions: Rc::new(RefCell::new(Vec::new())),
            empty_state,
            split_horizontal: Rc::new(RefCell::new(false)),
            phantom: Rc::new(RefCell::new(None)),
            saved_order: Rc::new(RefCell::new(None)),
        }
    }

    pub fn widget(&self) -> gtk::Grid {
        self.grid.clone()
    }

    pub fn empty_state_widget(&self) -> gtk::Box {
        self.empty_state.clone()
    }

    pub fn is_full(&self) -> bool {
        self.sessions.borrow().len() >= MAX_ACTIVE
    }

    pub fn count(&self) -> usize {
        self.sessions.borrow().len()
    }

    pub fn contains(&self, id: u32) -> bool {
        self.sessions.borrow().iter().any(|s| s.id() == id)
    }

    /// Adds a session to the arena. Returns the session that was evicted, if any.
    pub fn add(&self, session: Session) -> Option<Session> {
        let evicted = if self.is_full() {
            Some(self.sessions.borrow_mut().remove(0))
        } else {
            None
        };
        self.sessions.borrow_mut().push(session);
        self.rebuild();
        evicted
    }

    /// Removes a session from the arena; returns it if present.
    pub fn remove(&self, id: u32) -> Option<Session> {
        let mut v = self.sessions.borrow_mut();
        let pos = v.iter().position(|s| s.id() == id)?;
        let session = v.remove(pos);
        drop(v);
        self.rebuild();
        Some(session)
    }

    /// Swap the first arena session with `incoming`; returns the ejected session.
    pub fn swap_with_oldest(&self, incoming: Session) -> Option<Session> {
        if self.sessions.borrow().is_empty() {
            self.sessions.borrow_mut().push(incoming);
            self.rebuild();
            return None;
        }
        let evicted = self.sessions.borrow_mut().remove(0);
        self.sessions.borrow_mut().push(incoming);
        self.rebuild();
        Some(evicted)
    }

    /// Swap a specific arena session (by id) with `incoming`.
    pub fn swap_at(&self, target_id: u32, incoming: Session) -> Option<Session> {
        let mut v = self.sessions.borrow_mut();
        let pos = v.iter().position(|s| s.id() == target_id)?;
        let evicted = std::mem::replace(&mut v[pos], incoming);
        drop(v);
        self.rebuild();
        Some(evicted)
    }

    pub fn swap_sessions(&self, id_a: u32, id_b: u32) {
        let mut v = self.sessions.borrow_mut();
        let pos_a = v.iter().position(|s| s.id() == id_a);
        let pos_b = v.iter().position(|s| s.id() == id_b);
        if let (Some(a), Some(b)) = (pos_a, pos_b) {
            v.swap(a, b);
        }
        drop(v);
        self.rebuild();
    }

    pub fn toggle_split(&self) {
        let mut b = self.split_horizontal.borrow_mut();
        *b = !*b;
        drop(b);
        self.rebuild();
    }

    pub fn session_by_index(&self, index: usize) -> Option<Session> {
        self.sessions.borrow().get(index).cloned()
    }

    pub fn focused(&self) -> Option<Session> {
        self.sessions
            .borrow()
            .iter()
            .find(|s| s.is_focused())
            .cloned()
    }

    pub fn session_ids(&self) -> Vec<u32> {
        self.sessions.borrow().iter().map(|s| s.id()).collect()
    }

    /// Show a phantom placeholder in the arena, previewing the n+1 layout.
    /// The phantom appears in the last slot.
    pub fn show_phantom(&self) {
        if self.sessions.borrow().len() >= MAX_ACTIVE {
            return;
        }
        let phantom = self.get_or_create_phantom();
        phantom.set_visible(true);
        *self.phantom.borrow_mut() = Some(phantom);
        self.rebuild();
    }

    /// Hide the phantom placeholder and rebuild to normal layout.
    pub fn hide_phantom(&self) {
        let had_phantom = self.phantom.borrow().is_some();
        if let Some(phantom) = self.phantom.borrow().as_ref() {
            phantom.set_visible(false);
        }
        *self.phantom.borrow_mut() = None;
        if had_phantom {
            self.rebuild();
        }
    }

    /// Preview a swap between two arena sessions.
    pub fn preview_swap(&self, id_a: u32, id_b: u32) {
        let mut v = self.sessions.borrow_mut();
        // Save original order if not already saved.
        if self.saved_order.borrow().is_none() {
            *self.saved_order.borrow_mut() = Some(v.iter().map(|s| s.id()).collect());
        }
        let pos_a = v.iter().position(|s| s.id() == id_a);
        let pos_b = v.iter().position(|s| s.id() == id_b);
        if let (Some(a), Some(b)) = (pos_a, pos_b) {
            v.swap(a, b);
        }
        drop(v);
        self.rebuild();
    }

    /// Preview removal: temporarily hide a session and rebuild with n-1 layout.
    /// The session is NOT actually removed — it stays in the vec but its tile
    /// is excluded from the grid.
    pub fn preview_remove(&self, id: u32) {
        let v = self.sessions.borrow();
        if !v.iter().any(|s| s.id() == id) {
            return;
        }
        // Save original order if not already saved.
        if self.saved_order.borrow().is_none() {
            *self.saved_order.borrow_mut() = Some(v.iter().map(|s| s.id()).collect());
        }
        drop(v);
        self.rebuild_excluding(id);
    }

    /// Restore session order to what it was before preview_swap/preview_remove.
    pub fn restore_preview(&self) {
        let saved = self.saved_order.borrow_mut().take();
        if let Some(order) = saved {
            let mut v = self.sessions.borrow_mut();
            v.sort_by_key(|s| order.iter().position(|&oid| oid == s.id()).unwrap_or(usize::MAX));
            drop(v);
            self.rebuild();
        }
    }

    fn get_or_create_phantom(&self) -> gtk::Box {
        // Reuse existing phantom if we have one.
        if let Some(p) = self.phantom.borrow().as_ref() {
            return p.clone();
        }
        let phantom = gtk::Box::new(gtk::Orientation::Vertical, 8);
        phantom.add_css_class("orbit-drop-placeholder");
        phantom.set_hexpand(true);
        phantom.set_vexpand(true);
        phantom.set_halign(gtk::Align::Fill);
        phantom.set_valign(gtk::Align::Fill);

        let icon = gtk::Image::from_icon_name("list-add-symbolic");
        icon.set_pixel_size(32);
        icon.set_opacity(0.6);
        icon.set_valign(gtk::Align::End);
        icon.set_vexpand(true);

        let label = gtk::Label::new(Some("Drop here"));
        label.set_opacity(0.6);
        label.set_valign(gtk::Align::Start);
        label.set_vexpand(true);

        phantom.append(&icon);
        phantom.append(&label);
        phantom
    }

    /// Remove all children and reattach per the current session count.
    fn rebuild(&self) {
        self.clear_grid();

        let sessions = self.sessions.borrow();
        let phantom = self.phantom.borrow();
        let count = sessions.len();
        let has_phantom = phantom.is_some();
        let total = count + if has_phantom { 1 } else { 0 };

        if total == 0 {
            self.grid.attach(&self.empty_state, 0, 0, 2, 2);
            return;
        }

        // Collect widgets: session tile_frames + optional phantom at the end.
        let mut widgets: Vec<gtk::Widget> = sessions
            .iter()
            .map(|s| s.tile_frame().upcast::<gtk::Widget>())
            .collect();
        if let Some(ref p) = *phantom {
            widgets.push(p.clone().upcast::<gtk::Widget>());
        }

        let horizontal = *self.split_horizontal.borrow();
        self.attach_tiled(&widgets, horizontal);
    }

    /// Rebuild the grid excluding one session (for preview_remove).
    fn rebuild_excluding(&self, exclude_id: u32) {
        self.clear_grid();

        let sessions = self.sessions.borrow();
        let widgets: Vec<gtk::Widget> = sessions
            .iter()
            .filter(|s| s.id() != exclude_id)
            .map(|s| s.tile_frame().upcast::<gtk::Widget>())
            .collect();

        if widgets.is_empty() {
            self.grid.attach(&self.empty_state, 0, 0, 2, 2);
            return;
        }

        let horizontal = *self.split_horizontal.borrow();
        self.attach_tiled(&widgets, horizontal);
    }

    /// Remove all grid children.
    fn clear_grid(&self) {
        let mut child = self.grid.first_child();
        while let Some(w) = child {
            let next = w.next_sibling();
            self.grid.remove(&w);
            child = next;
        }
    }

    /// Attach a list of widgets to the grid using the standard tiling layout.
    fn attach_tiled(&self, widgets: &[gtk::Widget], horizontal: bool) {
        let count = widgets.len();
        match count {
            0 => {
                self.grid.attach(&self.empty_state, 0, 0, 2, 2);
            }
            1 => {
                self.grid.attach(&widgets[0], 0, 0, 2, 2);
            }
            2 => {
                if horizontal {
                    self.grid.attach(&widgets[0], 0, 0, 1, 2);
                    self.grid.attach(&widgets[1], 1, 0, 1, 2);
                } else {
                    self.grid.attach(&widgets[0], 0, 0, 2, 1);
                    self.grid.attach(&widgets[1], 0, 1, 2, 1);
                }
            }
            3 => {
                if horizontal {
                    self.grid.attach(&widgets[0], 0, 0, 1, 2);
                    self.grid.attach(&widgets[1], 1, 0, 1, 1);
                    self.grid.attach(&widgets[2], 1, 1, 1, 1);
                } else {
                    self.grid.attach(&widgets[0], 0, 0, 2, 1);
                    self.grid.attach(&widgets[1], 0, 1, 1, 1);
                    self.grid.attach(&widgets[2], 1, 1, 1, 1);
                }
            }
            _ => {
                // 4 or more: 2×2 grid (clamp at 4).
                for (i, w) in widgets.iter().take(4).enumerate() {
                    let col = (i % 2) as i32;
                    let row = (i / 2) as i32;
                    self.grid.attach(w, col, row, 1, 1);
                }
            }
        }
    }
}

fn build_empty_state() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 12);
    b.set_valign(gtk::Align::Center);
    b.set_halign(gtk::Align::Center);
    b.add_css_class("orbit-empty-state");

    let icon = gtk::Image::from_icon_name("utilities-terminal-symbolic");
    icon.set_pixel_size(64);
    icon.set_opacity(0.5);

    let title = gtk::Label::new(Some("Arena is empty"));
    title.add_css_class("title-2");

    let subtitle = gtk::Label::new(Some(
        "Promote a session from the sidebar, or press Ctrl+Shift+N for a new shell.",
    ));
    subtitle.set_wrap(true);
    subtitle.set_justify(gtk::Justification::Center);
    subtitle.set_max_width_chars(40);

    b.append(&icon);
    b.append(&title);
    b.append(&subtitle);
    b
}
