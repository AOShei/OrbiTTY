use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::session::Session;

/// Which subset of dock cards is visible.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    All,
    Busy,
    Attention,
}

/// The right-hand dock holding demoted session cards.
#[derive(Clone)]
pub struct Sidebar {
    pub root: gtk::Box,
    pub(crate) list: gtk::Box,
    pub(crate) scroller: gtk::ScrolledWindow,
    pub sessions: Rc<RefCell<Vec<Session>>>,
    /// Placeholder widget shown during drag-over.
    placeholder: Rc<RefCell<Option<gtk::Box>>>,
    /// The session id currently being dragged (for smart placeholder positioning).
    dragging_id: Rc<Cell<Option<u32>>>,
    filter_mode: Rc<Cell<FilterMode>>,
    edge_above: gtk::Button,
    edge_below: gtk::Button,
}

impl Sidebar {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("orbit-sidebar");
        root.set_width_request(260);

        // --- Header: filter toggle group ---
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        header.add_css_class("orbit-sidebar-header");
        header.set_margin_bottom(6);

        let filter_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        filter_box.add_css_class("linked");
        filter_box.set_hexpand(true);

        let btn_all = gtk::ToggleButton::with_label("All");
        btn_all.set_active(true);
        btn_all.set_hexpand(true);
        btn_all.add_css_class("orbit-filter-pill");
        let btn_busy = gtk::ToggleButton::with_label("Busy");
        btn_busy.set_group(Some(&btn_all));
        btn_busy.set_hexpand(true);
        btn_busy.add_css_class("orbit-filter-pill");
        let btn_attn = gtk::ToggleButton::with_label("Ready");
        btn_attn.set_group(Some(&btn_all));
        btn_attn.set_hexpand(true);
        btn_attn.add_css_class("orbit-filter-pill");

        filter_box.append(&btn_all);
        filter_box.append(&btn_busy);
        filter_box.append(&btn_attn);
        header.append(&filter_box);

        // --- Edge attention indicators ---
        let edge_above = gtk::Button::with_label("↑ 0");
        edge_above.add_css_class("orbit-attn-edge");
        edge_above.add_css_class("flat");
        edge_above.set_tooltip_text(Some("Attention above — click to scroll"));
        edge_above.set_visible(false);

        let edge_below = gtk::Button::with_label("↓ 0");
        edge_below.add_css_class("orbit-attn-edge");
        edge_below.add_css_class("flat");
        edge_below.set_tooltip_text(Some("Attention below — click to scroll"));
        edge_below.set_visible(false);

        let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
        list.set_hexpand(true);
        list.set_valign(gtk::Align::Start);

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .build();
        scroller.set_child(Some(&list));

        let scroll_area = gtk::Box::new(gtk::Orientation::Vertical, 0);
        scroll_area.set_vexpand(true);
        scroll_area.append(&edge_above);
        scroll_area.append(&scroller);
        scroll_area.append(&edge_below);

        root.append(&header);
        root.append(&scroll_area);

        let sidebar = Sidebar {
            root,
            list,
            scroller,
            sessions: Rc::new(RefCell::new(Vec::new())),
            placeholder: Rc::new(RefCell::new(None)),
            dragging_id: Rc::new(Cell::new(None)),
            filter_mode: Rc::new(Cell::new(FilterMode::All)),
            edge_above: edge_above.clone(),
            edge_below: edge_below.clone(),
        };

        // Wire filter toggles. Only fire on activation to ignore deactivation
        // signals that pair with the new selection (avoids double work).
        {
            let s = sidebar.clone();
            btn_all.connect_toggled(move |b| {
                if b.is_active() {
                    s.filter_mode.set(FilterMode::All);
                    s.apply_filter();
                    s.update_edge_indicators();
                }
            });
        }
        {
            let s = sidebar.clone();
            btn_busy.connect_toggled(move |b| {
                if b.is_active() {
                    s.filter_mode.set(FilterMode::Busy);
                    s.apply_filter();
                    s.update_edge_indicators();
                }
            });
        }
        {
            let s = sidebar.clone();
            btn_attn.connect_toggled(move |b| {
                if b.is_active() {
                    s.filter_mode.set(FilterMode::Attention);
                    s.apply_filter();
                    s.update_edge_indicators();
                }
            });
        }

        // Edge-indicator click → scroll to the nearest off-screen attention card.
        {
            let s = sidebar.clone();
            edge_above.connect_clicked(move |_| s.scroll_to_attention_above());
        }
        {
            let s = sidebar.clone();
            edge_below.connect_clicked(move |_| s.scroll_to_attention_below());
        }

        sidebar
    }

    /// Iterate sessions and set card visibility per the active filter.
    fn apply_filter(&self) {
        let mode = self.filter_mode.get();
        let sessions = self.sessions.borrow();
        for s in sessions.iter() {
            let show = match mode {
                FilterMode::All => true,
                FilterMode::Busy => s.is_busy(),
                FilterMode::Attention => s.has_attention(),
            };
            s.card_frame().set_visible(show);
        }
    }

    /// Count visible attention cards fully above / fully below the scroller's
    /// current viewport and update the edge indicator labels and visibility.
    fn update_edge_indicators(&self) {
        let adj = self.scroller.vadjustment();
        let view_top = adj.value();
        let view_bot = view_top + adj.page_size();

        let mut above = 0u32;
        let mut below = 0u32;

        let sessions = self.sessions.borrow();
        for s in sessions.iter() {
            if !s.has_attention() {
                continue;
            }
            let card = s.card_frame();
            if !card.is_visible() {
                continue;
            }
            let Some(p) = card.compute_point(&self.list, &gtk::graphene::Point::new(0.0, 0.0))
            else {
                continue;
            };
            let card_y = p.y() as f64;
            let card_h = card.height() as f64;
            if card_h <= 0.0 {
                continue; // not allocated yet
            }
            if card_y + card_h <= view_top {
                above += 1;
            } else if card_y >= view_bot {
                below += 1;
            }
        }

        if above > 0 {
            self.edge_above.set_label(&format!("↑ {} attention", above));
            self.edge_above.set_visible(true);
        } else {
            self.edge_above.set_visible(false);
        }
        if below > 0 {
            self.edge_below.set_label(&format!("↓ {} attention", below));
            self.edge_below.set_visible(true);
        } else {
            self.edge_below.set_visible(false);
        }
    }

    fn scroll_to_attention_above(&self) {
        let adj = self.scroller.vadjustment();
        let view_top = adj.value();
        let sessions = self.sessions.borrow();
        // Closest attention card whose bottom is <= view_top: the one with
        // the LARGEST y still below view_top.
        let mut best: Option<f64> = None;
        for s in sessions.iter() {
            if !s.has_attention() {
                continue;
            }
            let card = s.card_frame();
            if !card.is_visible() {
                continue;
            }
            let Some(p) = card.compute_point(&self.list, &gtk::graphene::Point::new(0.0, 0.0))
            else {
                continue;
            };
            let card_y = p.y() as f64;
            let card_h = card.height() as f64;
            if card_y + card_h <= view_top {
                if best.map_or(true, |b| card_y > b) {
                    best = Some(card_y);
                }
            }
        }
        if let Some(y) = best {
            adj.set_value(y);
        }
    }

    fn scroll_to_attention_below(&self) {
        let adj = self.scroller.vadjustment();
        let view_bot = adj.value() + adj.page_size();
        let sessions = self.sessions.borrow();
        // Closest attention card whose top is >= view_bot: smallest y above view_bot.
        let mut best: Option<f64> = None;
        for s in sessions.iter() {
            if !s.has_attention() {
                continue;
            }
            let card = s.card_frame();
            if !card.is_visible() {
                continue;
            }
            let Some(p) = card.compute_point(&self.list, &gtk::graphene::Point::new(0.0, 0.0))
            else {
                continue;
            };
            let card_y = p.y() as f64;
            if card_y >= view_bot {
                if best.map_or(true, |b| card_y < b) {
                    best = Some(card_y);
                }
            }
        }
        if let Some(y) = best {
            // Aim to align the card top near the viewport top.
            adj.set_value(y);
        }
    }

    pub(crate) fn tick(&self) {
        self.apply_filter();
        self.update_edge_indicators();
    }

    pub fn widget(&self) -> gtk::Box {
        self.root.clone()
    }

    pub fn scroller_widget(&self) -> gtk::ScrolledWindow {
        self.scroller.clone()
    }

    pub fn add(&self, session: Session) {
        self.list.append(&session.card_frame());
        self.sessions.borrow_mut().push(session);
    }

    pub fn remove(&self, id: u32) -> Option<Session> {
        let mut v = self.sessions.borrow_mut();
        let pos = v.iter().position(|s| s.id() == id)?;
        let session = v.remove(pos);
        drop(v);
        self.list.remove(&session.card_frame());
        Some(session)
    }

    pub fn contains(&self, id: u32) -> bool {
        self.sessions.borrow().iter().any(|s| s.id() == id)
    }

    pub fn session_ids(&self) -> Vec<u32> {
        self.sessions.borrow().iter().map(|s| s.id()).collect()
    }

    /// Reorder: move session `source_id` to the position of `target_id`.
    pub fn reorder_before(&self, source_id: u32, target_id: u32) {
        let mut v = self.sessions.borrow_mut();
        let Some(src_pos) = v.iter().position(|s| s.id() == source_id) else { return };
        let session = v.remove(src_pos);
        let tgt_pos = v.iter().position(|s| s.id() == target_id).unwrap_or(v.len());
        v.insert(tgt_pos, session.clone());
        drop(v);
        // Re-order the visual list: remove card and re-insert at the new position.
        self.list.remove(&session.card_frame());
        let sessions = self.sessions.borrow();
        if tgt_pos == 0 {
            self.list.prepend(&session.card_frame());
        } else if let Some(prev) = sessions.get(tgt_pos.saturating_sub(1)) {
            self.list.insert_child_after(&session.card_frame(), Some(&prev.card_frame()));
        } else {
            self.list.append(&session.card_frame());
        }
    }

    /// Reorder: move session `source_id` to the given index in the list.
    /// `index` is the target position in the original (pre-removal) list.
    pub fn reorder_to_index(&self, source_id: u32, index: usize) {
        let mut v = self.sessions.borrow_mut();
        let Some(src_pos) = v.iter().position(|s| s.id() == source_id) else { return };
        // Adjust target index: if source was before target, removing it shifts
        // everything after it down by one.
        let adjusted = if src_pos < index {
            index.saturating_sub(1)
        } else {
            index
        };
        let session = v.remove(src_pos);
        let clamped = adjusted.min(v.len());
        v.insert(clamped, session.clone());
        drop(v);
        self.list.remove(&session.card_frame());
        let sessions = self.sessions.borrow();
        if clamped == 0 {
            self.list.prepend(&session.card_frame());
        } else if let Some(prev) = sessions.get(clamped.saturating_sub(1)) {
            self.list.insert_child_after(&session.card_frame(), Some(&prev.card_frame()));
        } else {
            self.list.append(&session.card_frame());
        }
    }

    /// Return the insertion index that the placeholder is currently at.
    pub fn placeholder_insert_index(&self) -> usize {
        let placeholder = self.placeholder.borrow();
        let Some(ph) = placeholder.as_ref() else { return 0 };
        if ph.parent().is_none() {
            return 0;
        }
        // Walk the list children and count the position of the placeholder.
        let sessions = self.sessions.borrow();
        let mut idx = 0;
        for s in sessions.iter() {
            let card = s.card_frame();
            // If the placeholder comes before this card, we've found the index.
            if let Some(prev) = card.prev_sibling() {
                if prev == ph.clone().upcast::<gtk::Widget>() {
                    return idx;
                }
            }
            idx += 1;
        }
        idx
    }

    /// Set the session id currently being dragged within the sidebar.
    pub fn set_dragging_id(&self, id: u32) {
        self.dragging_id.set(Some(id));
    }

    /// Clear the dragging session id.
    pub fn clear_dragging_id(&self) {
        self.dragging_id.set(None);
    }

    /// Show a drop placeholder in the sidebar at the given y coordinate.
    /// The placeholder is inserted between existing cards at the closest gap.
    pub fn show_placeholder(&self, y: f64) {
        let placeholder = self.get_or_create_placeholder();
        if placeholder.parent().is_some() {
            self.list.remove(&placeholder);
        }

        let insert_idx = self.find_insert_index(y);
        // Suppress placeholder if it would land immediately before or after the
        // dragged card (those positions are no-ops).
        if let Some(drag_id) = self.dragging_id.get() {
            let sessions = self.sessions.borrow();
            if let Some(drag_pos) = sessions.iter().position(|s| s.id() == drag_id) {
                let effective = insert_idx.unwrap_or(0);
                // insert_idx None → position 0 (before first card)
                // insert_idx Some(i) → position i+1 (after card i)
                let target_pos = if insert_idx.is_none() { 0 } else { effective + 1 };
                if target_pos == drag_pos || target_pos == drag_pos + 1 {
                    // No-op position — hide placeholder.
                    placeholder.set_visible(false);
                    *self.placeholder.borrow_mut() = Some(placeholder);
                    return;
                }
            }
        }

        self.list.insert_child_after(&placeholder, self.child_at_index(insert_idx).as_ref());
        placeholder.set_visible(true);
        *self.placeholder.borrow_mut() = Some(placeholder);
    }

    /// Move the placeholder to the gap closest to the given y coordinate.
    pub fn move_placeholder(&self, y: f64) {
        let Some(placeholder) = self.placeholder.borrow().clone() else { return };
        if placeholder.parent().is_some() {
            self.list.remove(&placeholder);
        }
        let insert_idx = self.find_insert_index(y);

        if let Some(drag_id) = self.dragging_id.get() {
            let sessions = self.sessions.borrow();
            if let Some(drag_pos) = sessions.iter().position(|s| s.id() == drag_id) {
                let target_pos = if insert_idx.is_none() { 0 } else { insert_idx.unwrap() + 1 };
                if target_pos == drag_pos || target_pos == drag_pos + 1 {
                    placeholder.set_visible(false);
                    return;
                }
            }
        }

        self.list.insert_child_after(&placeholder, self.child_at_index(insert_idx).as_ref());
        placeholder.set_visible(true);
    }

    /// Hide and detach the placeholder.
    pub fn hide_placeholder(&self) {
        if let Some(placeholder) = self.placeholder.borrow().as_ref() {
            if placeholder.parent().is_some() {
                self.list.remove(placeholder);
            }
            placeholder.set_visible(false);
        }
        *self.placeholder.borrow_mut() = None;
    }

    fn get_or_create_placeholder(&self) -> gtk::Box {
        if let Some(p) = self.placeholder.borrow().as_ref() {
            return p.clone();
        }
        let placeholder = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        placeholder.add_css_class("orbit-drop-placeholder");
        placeholder.add_css_class("orbit-sidebar-placeholder");
        placeholder.set_hexpand(true);
        placeholder.set_size_request(-1, 50);
        placeholder
    }

    /// Find which child index to insert the placeholder before.
    /// Returns None if it should go at the start, or the child widget
    /// to insert after.
    fn find_insert_index(&self, y: f64) -> Option<usize> {
        let sessions = self.sessions.borrow();
        if sessions.is_empty() {
            return None;
        }
        for (i, s) in sessions.iter().enumerate() {
            let card = s.card_frame();
            let (_, card_y) = card
                .compute_point(&self.list, &gtk::graphene::Point::new(0.0, 0.0))
                .map(|p| (p.x() as f64, p.y() as f64))
                .unwrap_or((0.0, 0.0));
            let midpoint = card_y + (card.height() as f64 / 2.0);
            if y < midpoint {
                return if i == 0 { None } else { Some(i - 1) };
            }
        }
        Some(sessions.len() - 1)
    }

    /// Get the Nth card frame child for use with insert_child_after.
    fn child_at_index(&self, index: Option<usize>) -> Option<gtk::Widget> {
        let Some(idx) = index else { return None };
        let sessions = self.sessions.borrow();
        sessions.get(idx).map(|s| s.card_frame().upcast::<gtk::Widget>())
    }
}
