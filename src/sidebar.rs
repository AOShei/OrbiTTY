use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::RefCell;
use std::rc::Rc;

use crate::session::Session;

/// The right-hand "Monitoring Sidebar" holding inactive session cards.
#[derive(Clone)]
pub struct Sidebar {
    pub root: gtk::Box,
    pub(crate) list: gtk::Box,
    pub(crate) scroller: gtk::ScrolledWindow,
    pub sessions: Rc<RefCell<Vec<Session>>>,
    /// Placeholder widget shown during drag-over.
    placeholder: Rc<RefCell<Option<gtk::Box>>>,
}

impl Sidebar {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("orbit-sidebar");
        root.set_width_request(260);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header.set_margin_bottom(6);

        let title = gtk::Label::new(Some("Monitoring"));
        title.add_css_class("heading");
        title.set_halign(gtk::Align::Start);
        title.set_hexpand(true);

        header.append(&title);

        let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
        list.set_hexpand(true);
        list.set_valign(gtk::Align::Start);

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .build();
        scroller.set_child(Some(&list));

        root.append(&header);
        root.append(&scroller);

        Sidebar {
            root,
            list,
            scroller,
            sessions: Rc::new(RefCell::new(Vec::new())),
            placeholder: Rc::new(RefCell::new(None)),
        }
    }

    pub fn widget(&self) -> gtk::Box {
        self.root.clone()
    }

    pub fn list_widget(&self) -> gtk::Box {
        self.list.clone()
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
    pub fn reorder_to_index(&self, source_id: u32, index: usize) {
        let mut v = self.sessions.borrow_mut();
        let Some(src_pos) = v.iter().position(|s| s.id() == source_id) else { return };
        let session = v.remove(src_pos);
        let clamped = index.min(v.len());
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

    #[allow(dead_code)]
    pub fn scroller(&self) -> gtk::ScrolledWindow {
        self.scroller.clone()
    }

    /// Show a drop placeholder in the sidebar at the given y coordinate.
    /// The placeholder is inserted between existing cards at the closest gap.
    pub fn show_placeholder(&self, y: f64) {
        let placeholder = self.get_or_create_placeholder();
        // Remove from current position before reinserting.
        if placeholder.parent().is_some() {
            self.list.remove(&placeholder);
        }

        let insert_idx = self.find_insert_index(y);
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
        self.list.insert_child_after(&placeholder, self.child_at_index(insert_idx).as_ref());
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
