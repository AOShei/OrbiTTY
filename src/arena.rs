use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::{Cell, RefCell};
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
    /// Which slot index the phantom occupies (for position-aware insertion).
    phantom_index: Rc<Cell<usize>>,
    /// Session temporarily hidden for arena-shrink preview (drag to sidebar).
    preview_removed: Rc<RefCell<Option<u32>>>,
    /// Pair of session ids currently preview-swapped (arena→arena drag).
    preview_swapped: Rc<RefCell<Option<(u32, u32)>>>,
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
            phantom_index: Rc::new(Cell::new(0)),
            preview_removed: Rc::new(RefCell::new(None)),
            preview_swapped: Rc::new(RefCell::new(None)),
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

    /// Show or move phantom to the given slot. Avoids unnecessary rebuilds.
    pub fn ensure_phantom_at(&self, slot: usize) {
        if self.sessions.borrow().len() >= MAX_ACTIVE {
            return;
        }
        if self.phantom.borrow().is_some() {
            self.move_phantom_to(slot);
        } else {
            self.show_phantom_at(slot);
        }
    }

    /// Show a phantom placeholder at the given slot index, previewing the n+1 layout.
    pub fn show_phantom_at(&self, slot: usize) {
        if self.sessions.borrow().len() >= MAX_ACTIVE {
            return;
        }
        let phantom = self.get_or_create_phantom();
        phantom.set_visible(true);
        *self.phantom.borrow_mut() = Some(phantom);
        self.phantom_index.set(slot);
        self.rebuild();
    }

    /// Update phantom position without recreating it. No-op if phantom is not shown.
    pub fn move_phantom_to(&self, slot: usize) {
        if self.phantom.borrow().is_none() {
            return;
        }
        if self.phantom_index.get() == slot {
            return;
        }
        self.phantom_index.set(slot);
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

    /// Return the slot index where the phantom is currently shown.
    pub fn phantom_slot(&self) -> usize {
        self.phantom_index.get()
    }

    /// Return whether the phantom is currently shown.
    pub fn has_phantom(&self) -> bool {
        self.phantom.borrow().is_some()
    }

    // --- Arena shrink preview (drag arena tile → sidebar) ---

    /// Hide a session from the arena layout to preview what n-1 looks like.
    pub fn preview_remove(&self, id: u32) {
        if *self.preview_removed.borrow() == Some(id) {
            return;
        }
        *self.preview_removed.borrow_mut() = Some(id);
        self.rebuild();
    }

    /// Restore a session hidden by `preview_remove`.
    pub fn restore_remove(&self) {
        if self.preview_removed.borrow_mut().take().is_some() {
            self.rebuild();
        }
    }

    /// Whether a remove preview is active.
    pub fn has_preview_remove(&self) -> bool {
        self.preview_removed.borrow().is_some()
    }

    // --- Arena swap preview (arena → arena drag) ---

    /// Preview swapping two arena sessions. Undoes any previous preview swap
    /// and performs a single rebuild.
    pub fn preview_swap(&self, id_a: u32, id_b: u32) {
        {
            let current = self.preview_swapped.borrow();
            if let Some((a, b)) = *current {
                if (a == id_a && b == id_b) || (a == id_b && b == id_a) {
                    return; // Already previewing this swap.
                }
            }
        }
        // Undo the previous preview swap in-place (no rebuild yet).
        {
            let old = self.preview_swapped.borrow_mut().take();
            if let Some((old_a, old_b)) = old {
                let mut v = self.sessions.borrow_mut();
                let pos_a = v.iter().position(|s| s.id() == old_a);
                let pos_b = v.iter().position(|s| s.id() == old_b);
                if let (Some(a), Some(b)) = (pos_a, pos_b) {
                    v.swap(a, b);
                }
            }
        }
        // Apply the new preview swap.
        {
            let mut v = self.sessions.borrow_mut();
            let pos_a = v.iter().position(|s| s.id() == id_a);
            let pos_b = v.iter().position(|s| s.id() == id_b);
            if let (Some(a), Some(b)) = (pos_a, pos_b) {
                v.swap(a, b);
                drop(v);
                *self.preview_swapped.borrow_mut() = Some((id_a, id_b));
                self.rebuild();
            }
        }
    }

    /// Undo a preview swap, restoring original positions.
    pub fn undo_preview_swap(&self) {
        let ids = self.preview_swapped.borrow_mut().take();
        if let Some((id_a, id_b)) = ids {
            let mut v = self.sessions.borrow_mut();
            let pos_a = v.iter().position(|s| s.id() == id_a);
            let pos_b = v.iter().position(|s| s.id() == id_b);
            if let (Some(a), Some(b)) = (pos_a, pos_b) {
                v.swap(a, b);
            }
            drop(v);
            self.rebuild();
        }
    }

    /// Commit the preview swap (keep current positions, clear preview state).
    pub fn commit_preview_swap(&self) {
        *self.preview_swapped.borrow_mut() = None;
    }

    /// Whether a swap preview is active.
    pub fn has_preview_swap(&self) -> bool {
        self.preview_swapped.borrow().is_some()
    }

    /// Clear all preview states (phantom, remove, swap).
    pub fn clear_all_previews(&self) {
        let had_any = self.phantom.borrow().is_some()
            || self.preview_removed.borrow().is_some()
            || self.preview_swapped.borrow().is_some();

        // Hide phantom.
        if let Some(phantom) = self.phantom.borrow().as_ref() {
            phantom.set_visible(false);
        }
        *self.phantom.borrow_mut() = None;

        // Restore removed session.
        *self.preview_removed.borrow_mut() = None;

        // Undo swap in-place.
        {
            let ids = self.preview_swapped.borrow_mut().take();
            if let Some((id_a, id_b)) = ids {
                let mut v = self.sessions.borrow_mut();
                let pos_a = v.iter().position(|s| s.id() == id_a);
                let pos_b = v.iter().position(|s| s.id() == id_b);
                if let (Some(a), Some(b)) = (pos_a, pos_b) {
                    v.swap(a, b);
                }
            }
        }

        if had_any {
            self.rebuild();
        }
    }

    /// Compute which slot index a point (x, y) in grid coordinates maps to,
    /// given that the total tile count will be `current_sessions + 1` (phantom).
    pub fn slot_from_coords(&self, x: f64, y: f64) -> usize {
        let w = self.grid.width() as f64;
        let h = self.grid.height() as f64;
        if w <= 0.0 || h <= 0.0 {
            return 0;
        }
        let horiz = *self.split_horizontal.borrow();
        let count = self.sessions.borrow().len();
        let total = count + 1; // including phantom
        match total {
            0 | 1 => 0,
            2 => {
                if horiz {
                    if x < w / 2.0 { 0 } else { 1 }
                } else {
                    if y < h / 2.0 { 0 } else { 1 }
                }
            }
            3 => {
                if horiz {
                    // col 0 = big, col 1 top/bottom = small
                    if x < w / 2.0 { 0 }
                    else if y < h / 2.0 { 1 }
                    else { 2 }
                } else {
                    // row 0 = big, row 1 left/right = small
                    if y < h / 2.0 { 0 }
                    else if x < w / 2.0 { 1 }
                    else { 2 }
                }
            }
            _ => {
                // 2×2
                let col = if x < w / 2.0 { 0 } else { 1 };
                let row = if y < h / 2.0 { 0 } else { 1 };
                (row * 2 + col) as usize
            }
        }
    }

    /// Insert a session at a specific index in the arena. Clamps to valid range.
    pub fn insert_at(&self, index: usize, session: Session) {
        let mut v = self.sessions.borrow_mut();
        let clamped = index.min(v.len());
        v.insert(clamped, session);
        drop(v);
        self.rebuild();
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
        let removed_id = *self.preview_removed.borrow();

        // Collect visible session tile_frames (skip preview-removed session).
        let mut widgets: Vec<gtk::Widget> = sessions
            .iter()
            .filter(|s| removed_id.map_or(true, |rid| s.id() != rid))
            .map(|s| s.tile_frame().upcast::<gtk::Widget>())
            .collect();

        let has_phantom = phantom.is_some();
        let total = widgets.len() + if has_phantom { 1 } else { 0 };

        if total == 0 {
            self.grid.attach(&self.empty_state, 0, 0, 2, 2);
            return;
        }

        // Insert phantom at the tracked position.
        if let Some(ref p) = *phantom {
            let idx = self.phantom_index.get().min(widgets.len());
            widgets.insert(idx, p.clone().upcast::<gtk::Widget>());
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
