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
        }
    }

    pub fn widget(&self) -> gtk::Grid {
        self.grid.clone()
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

    /// Remove all children and reattach per the current session count.
    fn rebuild(&self) {
        // Remove everything currently in the grid.
        let mut child = self.grid.first_child();
        while let Some(w) = child {
            let next = w.next_sibling();
            self.grid.remove(&w);
            child = next;
        }

        let sessions = self.sessions.borrow();
        let count = sessions.len();

        if count == 0 {
            self.grid.attach(&self.empty_state, 0, 0, 2, 2);
            return;
        }

        let horizontal = *self.split_horizontal.borrow();

        match count {
            1 => {
                let s = &sessions[0];
                self.grid.attach(&s.tile_frame(), 0, 0, 2, 2);
            }
            2 => {
                let s1 = &sessions[0];
                let s2 = &sessions[1];
                if horizontal {
                    // top / bottom
                    self.grid.attach(&s1.tile_frame(), 0, 0, 2, 1);
                    self.grid.attach(&s2.tile_frame(), 0, 1, 2, 1);
                } else {
                    // left / right
                    self.grid.attach(&s1.tile_frame(), 0, 0, 1, 2);
                    self.grid.attach(&s2.tile_frame(), 1, 0, 1, 2);
                }
            }
            3 => {
                let s1 = &sessions[0];
                let s2 = &sessions[1];
                let s3 = &sessions[2];
                if horizontal {
                    // Big top, two small bottom.
                    self.grid.attach(&s1.tile_frame(), 0, 0, 2, 1);
                    self.grid.attach(&s2.tile_frame(), 0, 1, 1, 1);
                    self.grid.attach(&s3.tile_frame(), 1, 1, 1, 1);
                } else {
                    // Big left, two small right.
                    self.grid.attach(&s1.tile_frame(), 0, 0, 1, 2);
                    self.grid.attach(&s2.tile_frame(), 1, 0, 1, 1);
                    self.grid.attach(&s3.tile_frame(), 1, 1, 1, 1);
                }
            }
            4 => {
                self.grid.attach(&sessions[0].tile_frame(), 0, 0, 1, 1);
                self.grid.attach(&sessions[1].tile_frame(), 1, 0, 1, 1);
                self.grid.attach(&sessions[2].tile_frame(), 0, 1, 1, 1);
                self.grid.attach(&sessions[3].tile_frame(), 1, 1, 1, 1);
            }
            _ => {
                // Shouldn't happen; clamp by dropping excess.
                for (i, s) in sessions.iter().take(4).enumerate() {
                    let col = (i % 2) as i32;
                    let row = (i / 2) as i32;
                    self.grid.attach(&s.tile_frame(), col, row, 1, 1);
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
