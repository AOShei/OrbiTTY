use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use crate::arena::{Arena, MAX_ACTIVE};
use crate::session::{Session, SessionCallback, SessionEvent};
use crate::sidebar::Sidebar;
use crate::templates::WorkspaceTemplate;

pub struct WorkspaceInner {
    pub id: u32,
    pub name: String,
    pub template_name: String,
    pub root: gtk::Paned,
    pub arena: Arena,
    pub sidebar: Sidebar,
    /// All sessions, indexed in spawn order. The location of each is owned by Session.
    pub registry: Vec<Session>,
    pub next_session_id: u32,
    /// Last focused session id, for restoring focus on tab switch.
    pub last_focused_id: Option<u32>,
}

#[derive(Clone)]
pub struct Workspace {
    pub inner: Rc<RefCell<WorkspaceInner>>,
}

impl Workspace {
    pub fn new(id: u32, name: &str, template: &WorkspaceTemplate) -> Self {
        let arena = Arena::new();
        let sidebar = Sidebar::new();

        let root = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .resize_start_child(true)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .position(900)
            .build();
        root.set_start_child(Some(&arena.widget()));
        root.set_end_child(Some(&sidebar.widget()));
        root.set_hexpand(true);
        root.set_vexpand(true);

        let inner = Rc::new(RefCell::new(WorkspaceInner {
            id,
            name: name.to_string(),
            template_name: template.name.to_string(),
            root,
            arena,
            sidebar,
            registry: Vec::new(),
            next_session_id: 1,
            last_focused_id: None,
        }));

        let workspace = Workspace { inner };

        // Build sessions from the template.
        for spec in &template.sessions {
            workspace.spawn_session(&spec.name, None, spec.promote);
        }

        // Focus first arena session if any.
        let first_id = workspace.inner.borrow().arena.session_by_index(0).map(|s| s.id());
        if let Some(id) = first_id {
            workspace.focus_session(id);
        }

        workspace
    }

    pub fn widget(&self) -> gtk::Paned {
        self.inner.borrow().root.clone()
    }

    pub fn name(&self) -> String {
        self.inner.borrow().name.clone()
    }

    pub fn id(&self) -> u32 {
        self.inner.borrow().id
    }

    /// Create a new session, auto-promote if requested and arena has space.
    pub fn spawn_session(&self, name: &str, cwd: Option<&str>, promote: bool) -> Session {
        let sid = {
            let mut inner = self.inner.borrow_mut();
            let id = inner.next_session_id;
            inner.next_session_id += 1;
            id
        };

        let cb = self.make_session_callback();
        let session = Session::new(sid, name, cwd, cb);

        self.inner.borrow_mut().registry.push(session.clone());

        if promote && !self.inner.borrow().arena.is_full() {
            self.inner.borrow().arena.add(session.clone());
            session.place_in_arena();
        } else {
            self.inner.borrow().sidebar.add(session.clone());
            session.place_in_sidebar();
        }

        session
    }

    fn make_session_callback(&self) -> SessionCallback {
        let weak: Weak<RefCell<WorkspaceInner>> = Rc::downgrade(&self.inner);
        Rc::new(RefCell::new(Box::new(move |id: u32, event: SessionEvent| {
            let Some(inner_rc) = weak.upgrade() else {
                return;
            };
            let ws = Workspace { inner: inner_rc };
            match event {
                SessionEvent::RequestPromote => ws.promote(id),
                SessionEvent::RequestDemote => ws.demote(id),
                SessionEvent::RequestClose => ws.close_session(id),
                SessionEvent::RequestClone => ws.clone_session(id),
                SessionEvent::RequestSwap(source_id) => {
                    let arena = ws.inner.borrow().arena.clone();
                    arena.swap_sessions(source_id, id);
                }
                SessionEvent::Focused => ws.focus_session(id),
                SessionEvent::Bell => {
                    if let Some(s) = ws.find(id) {
                        s.raise_alert();
                    }
                }
            }
        })))
    }

    pub fn find(&self, id: u32) -> Option<Session> {
        self.inner
            .borrow()
            .registry
            .iter()
            .find(|s| s.id() == id)
            .cloned()
    }

    /// Promote a sidebar session into the arena. If arena is full, swap with
    /// the currently focused arena session (fallback: oldest).
    pub fn promote(&self, id: u32) {
        let inner = self.inner.borrow();
        if inner.arena.contains(id) {
            // Already in arena: just focus.
            drop(inner);
            self.focus_session(id);
            return;
        }
        let sidebar = inner.sidebar.clone();
        let arena = inner.arena.clone();
        drop(inner);

        let Some(incoming) = sidebar.remove(id) else {
            return;
        };

        if arena.count() < MAX_ACTIVE {
            arena.add(incoming.clone());
            incoming.place_in_arena();
            self.focus_session(incoming.id());
            return;
        }

        // Arena full: swap with focused or oldest.
        let target_id = arena
            .focused()
            .map(|s| s.id())
            .or_else(|| arena.session_ids().first().copied());

        if let Some(tid) = target_id {
            if let Some(evicted) = arena.swap_at(tid, incoming.clone()) {
                incoming.place_in_arena();
                self.inner.borrow().sidebar.add(evicted.clone());
                evicted.place_in_sidebar();
            }
        } else {
            arena.add(incoming.clone());
            incoming.place_in_arena();
        }
        self.focus_session(incoming.id());
    }

    pub fn demote(&self, id: u32) {
        let inner = self.inner.borrow();
        let arena = inner.arena.clone();
        let sidebar = inner.sidebar.clone();
        drop(inner);

        if let Some(s) = arena.remove(id) {
            sidebar.add(s.clone());
            s.place_in_sidebar();
        }
    }

    pub fn toggle(&self, id: u32) {
        let inner = self.inner.borrow();
        let in_arena = inner.arena.contains(id);
        drop(inner);
        if in_arena {
            self.demote(id);
        } else {
            self.promote(id);
        }
    }

    pub fn focus_session(&self, id: u32) {
        self.inner.borrow_mut().last_focused_id = Some(id);

        // Clone sessions out so the borrow is dropped before any GTK calls
        // (which can re-enter via signal handlers).
        let sessions: Vec<Session> = self.inner.borrow().registry.clone();
        for s in &sessions {
            s.set_focused(s.id() == id);
        }
        if let Some(s) = sessions.iter().find(|s| s.id() == id) {
            s.grab_focus();
        }
    }

    /// Restore focus to the last-focused session, or fall back to first arena session.
    pub fn refocus(&self) {
        let last = self.inner.borrow().last_focused_id;
        if let Some(id) = last {
            // Only refocus if the session still exists.
            if self.find(id).is_some() {
                self.focus_session(id);
                return;
            }
        }
        self.focus_index(1);
    }

    /// Focus the arena session at a 1-based index.
    pub fn focus_index(&self, idx_1based: usize) {
        let inner = self.inner.borrow();
        if idx_1based == 0 {
            return;
        }
        if let Some(s) = inner.arena.session_by_index(idx_1based - 1) {
            let id = s.id();
            drop(inner);
            self.focus_session(id);
        }
    }

    /// Toggle the arena session at a 1-based index between arena and sidebar.
    pub fn toggle_index(&self, idx_1based: usize) {
        let inner = self.inner.borrow();
        if idx_1based == 0 {
            return;
        }
        if let Some(s) = inner.arena.session_by_index(idx_1based - 1) {
            let id = s.id();
            drop(inner);
            self.toggle(id);
        } else {
            let inner = self.inner.borrow();
            let sidebar_ids = inner.sidebar.session_ids();
            if let Some(id) = sidebar_ids
                .get(idx_1based - 1 - inner.arena.count())
                .copied()
            {
                drop(inner);
                self.promote(id);
            }
        }
    }

    pub fn toggle_split(&self) {
        self.inner.borrow().arena.toggle_split();
    }

    pub fn new_session(&self) {
        let name = format!("Shell {}", self.inner.borrow().next_session_id);
        let home = std::env::var("HOME").ok();
        self.spawn_session(&name, home.as_deref(), true);
    }

    /// Spawn a new session with its cwd copied from an existing one.
    pub fn clone_session(&self, id: u32) {
        let cwd = self.find(id).and_then(|s| s.current_dir());
        let fallback = std::env::var("HOME").ok();
        let dir = cwd.as_deref().or(fallback.as_deref());
        let name = format!("Shell {}", self.inner.borrow().next_session_id);
        self.spawn_session(&name, dir, true);
    }

    /// Tear down a session: remove from arena/sidebar and drop from registry.
    pub fn close_session(&self, id: u32) {
        let (arena, sidebar) = {
            let inner = self.inner.borrow();
            (inner.arena.clone(), inner.sidebar.clone())
        };
        if arena.contains(id) {
            arena.remove(id);
        } else {
            sidebar.remove(id);
        }
        self.inner.borrow_mut().registry.retain(|s| s.id() != id);
    }

    pub fn sessions(&self) -> Vec<Session> {
        self.inner.borrow().registry.clone()
    }
}

