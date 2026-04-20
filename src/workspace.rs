use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::{Cell, RefCell};
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
    /// Guard flag: suppress DragHoverEnter/Leave events while a preview rebuild
    /// is in progress (rebuild triggers spurious DropTarget enter/leave signals).
    pub suppressing_hover: Rc<Cell<bool>>,
    /// Generation counter for drag hover events; stale leave callbacks skip
    /// their clear when a newer enter has already incremented the counter.
    pub drag_hover_gen: Rc<Cell<u32>>,
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
        root.add_css_class("orbit-workspace");

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
            suppressing_hover: Rc::new(Cell::new(false)),
            drag_hover_gen: Rc::new(Cell::new(0)),
        }));

        let workspace = Workspace { inner };

        // Background drop targets:
        //   - Arena grid: catch drops/hovers not on a specific tile.
        //   - Arena empty-state: drop here promotes the dragged session.
        //   - Sidebar list: drop on blank space demotes; hover shows placeholder.
        {
            let (arena_grid, empty_state, list) = {
                let inner = workspace.inner.borrow();
                (
                    inner.arena.widget(),
                    inner.arena.empty_state_widget(),
                    inner.sidebar.list_widget(),
                )
            };
            // Arena grid: hover shows phantom when sidebar card is dragged over.
            {
                let weak: Weak<RefCell<WorkspaceInner>> = Rc::downgrade(&workspace.inner);
                let dt = gtk::DropTarget::new(
                    <u32 as glib::types::StaticType>::static_type(),
                    gtk::gdk::DragAction::MOVE,
                );
                {
                    let weak = weak.clone();
                    dt.connect_enter(move |dt, x, y| {
                        if let Some(inner_rc) = weak.upgrade() {
                            let guard = inner_rc.borrow().suppressing_hover.clone();
                            if guard.get() { return gtk::gdk::DragAction::MOVE; }
                            let arena = inner_rc.borrow().arena.clone();
                            // Only show phantom for sidebar→arena drags, not arena→arena.
                            let source_in_arena = crate::session::extract_source_id(dt)
                                .map(|sid| arena.contains(sid))
                                .unwrap_or(false);
                            if source_in_arena { return gtk::gdk::DragAction::MOVE; }
                            let gen_rc = inner_rc.borrow().drag_hover_gen.clone();
                            gen_rc.set(gen_rc.get().wrapping_add(1));
                            let gen = gen_rc.get();
                            if !arena.is_full() {
                                let slot = arena.slot_from_coords(x, y);
                                glib::idle_add_local_once(move || {
                                    if gen_rc.get() != gen { return; }
                                    guard.set(true);
                                    arena.ensure_phantom_at(slot);
                                    // Defer guard unset so post-rebuild events are suppressed.
                                    glib::idle_add_local_once(move || {
                                        guard.set(false);
                                    });
                                });
                            }
                        }
                        gtk::gdk::DragAction::MOVE
                    });
                }
                {
                    let weak = weak.clone();
                    dt.connect_motion(move |dt, x, y| {
                        if let Some(inner_rc) = weak.upgrade() {
                            let guard = inner_rc.borrow().suppressing_hover.clone();
                            if guard.get() { return gtk::gdk::DragAction::MOVE; }
                            let arena = inner_rc.borrow().arena.clone();
                            let source_in_arena = crate::session::extract_source_id(dt)
                                .map(|sid| arena.contains(sid))
                                .unwrap_or(false);
                            if source_in_arena { return gtk::gdk::DragAction::MOVE; }
                            if !arena.is_full() {
                                let slot = arena.slot_from_coords(x, y);
                                if slot != arena.phantom_slot() || !arena.has_phantom() {
                                    let gen_rc = inner_rc.borrow().drag_hover_gen.clone();
                                    let gen = gen_rc.get();
                                    glib::idle_add_local_once(move || {
                                        if gen_rc.get() != gen { return; }
                                        guard.set(true);
                                        arena.ensure_phantom_at(slot);
                                        glib::idle_add_local_once(move || {
                                            guard.set(false);
                                        });
                                    });
                                }
                            }
                        }
                        gtk::gdk::DragAction::MOVE
                    });
                }
                {
                    let weak = weak.clone();
                    dt.connect_leave(move |_dt| {
                        if let Some(inner_rc) = weak.upgrade() {
                            let guard = inner_rc.borrow().suppressing_hover.clone();
                            if guard.get() { return; }
                            let gen_rc = inner_rc.borrow().drag_hover_gen.clone();
                            let gen = gen_rc.get();
                            let ws = Workspace { inner: inner_rc };
                            // Use a short timeout so that a subsequent enter (which
                            // bumps gen) can cancel this clear before it executes.
                            glib::timeout_add_local_once(
                                std::time::Duration::from_millis(50),
                                move || {
                                    if gen_rc.get() != gen { return; }
                                    guard.set(true);
                                    ws.clear_all_previews();
                                    guard.set(false);
                                },
                            );
                        }
                    });
                }
                {
                    let weak = weak.clone();
                    dt.connect_drop(move |_t, value, _x, _y| {
                        let Ok(source_id) = value.get::<u32>() else {
                            return false;
                        };
                        if let Some(inner_rc) = weak.upgrade() {
                            let ws = Workspace { inner: inner_rc };
                            let (arena, sidebar) = {
                                let inner = ws.inner.borrow();
                                (inner.arena.clone(), inner.sidebar.clone())
                            };
                            let slot = arena.phantom_slot();
                            ws.clear_all_previews();
                            if sidebar.contains(source_id) {
                                ws.promote_at(source_id, Some(slot));
                            } else if arena.contains(source_id) {
                                // Arena session dropped on grid background: no-op
                                // (arena→arena swap handled by per-tile DropTarget).
                            }
                        }
                        true
                    });
                }
                arena_grid.add_controller(dt);
            }
            // Arena empty-state: drop promotes.
            {
                let weak: Weak<RefCell<WorkspaceInner>> = Rc::downgrade(&workspace.inner);
                let dt = gtk::DropTarget::new(
                    <u32 as glib::types::StaticType>::static_type(),
                    gtk::gdk::DragAction::MOVE,
                );
                dt.connect_drop(move |_t, value, _x, _y| {
                    let Ok(source_id) = value.get::<u32>() else { return false };
                    if let Some(inner_rc) = weak.upgrade() {
                        let ws = Workspace { inner: inner_rc };
                        ws.clear_all_previews();
                        ws.promote(source_id);
                    }
                    true
                });
                empty_state.add_controller(dt);
            }
            // Sidebar list: hover shows placeholder, drop demotes.
            {
                let weak: Weak<RefCell<WorkspaceInner>> = Rc::downgrade(&workspace.inner);
                let dt = gtk::DropTarget::new(
                    <u32 as glib::types::StaticType>::static_type(),
                    gtk::gdk::DragAction::MOVE,
                );
                {
                    let weak = weak.clone();
                    dt.connect_enter(move |_dt, _x, y| {
                        if let Some(inner_rc) = weak.upgrade() {
                            let guard = inner_rc.borrow().suppressing_hover.clone();
                            if guard.get() { return gtk::gdk::DragAction::MOVE; }
                            let sidebar = inner_rc.borrow().sidebar.clone();
                            let gen_rc = inner_rc.borrow().drag_hover_gen.clone();
                            gen_rc.set(gen_rc.get().wrapping_add(1));
                            let gen = gen_rc.get();
                            glib::idle_add_local_once(move || {
                                if gen_rc.get() != gen { return; }
                                guard.set(true);
                                sidebar.show_placeholder(y);
                                guard.set(false);
                            });
                        }
                        gtk::gdk::DragAction::MOVE
                    });
                }
                {
                    let weak = weak.clone();
                    dt.connect_motion(move |_dt, _x, y| {
                        if let Some(inner_rc) = weak.upgrade() {
                            let guard = inner_rc.borrow().suppressing_hover.clone();
                            if guard.get() { return gtk::gdk::DragAction::MOVE; }
                            let sidebar = inner_rc.borrow().sidebar.clone();
                            sidebar.move_placeholder(y);
                        }
                        gtk::gdk::DragAction::MOVE
                    });
                }
                {
                    let weak = weak.clone();
                    dt.connect_leave(move |_dt| {
                        if let Some(inner_rc) = weak.upgrade() {
                            let guard = inner_rc.borrow().suppressing_hover.clone();
                            if guard.get() { return; }
                            let gen_rc = inner_rc.borrow().drag_hover_gen.clone();
                            let gen = gen_rc.get();
                            let ws = Workspace { inner: inner_rc };
                            glib::idle_add_local_once(move || {
                                if gen_rc.get() != gen { return; }
                                guard.set(true);
                                ws.clear_all_previews();
                                guard.set(false);
                            });
                        }
                    });
                }
                {
                    let weak = weak.clone();
                    dt.connect_drop(move |_t, value, _x, _y| {
                        let Ok(source_id) = value.get::<u32>() else { return false };
                        if let Some(inner_rc) = weak.upgrade() {
                            let ws = Workspace { inner: inner_rc };
                            let (arena, sidebar) = {
                                let inner = ws.inner.borrow();
                                (inner.arena.clone(), inner.sidebar.clone())
                            };
                            // Capture insertion index from placeholder before clearing.
                            let insert_idx = sidebar.placeholder_insert_index();
                            ws.clear_all_previews();
                            if arena.contains(source_id) {
                                ws.demote(source_id);
                            } else if sidebar.contains(source_id) {
                                sidebar.reorder_to_index(source_id, insert_idx);
                            }
                        }
                        true
                    });
                }
                list.add_controller(dt);
            }
        }

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
                SessionEvent::RequestSwap(source_id) => ws.handle_drop(source_id, id),
                SessionEvent::Focused => ws.focus_session(id),
                SessionEvent::Bell => {
                    if let Some(s) = ws.find(id) {
                        s.raise_alert();
                    }
                }
                SessionEvent::DragStarted => {
                    ws.inner.borrow().root.add_css_class("dragging");
                }
                SessionEvent::DragEnded => {
                    ws.inner.borrow().root.remove_css_class("dragging");
                    // Clean up any lingering drop-hover classes.
                    let sessions: Vec<Session> = ws.inner.borrow().registry.clone();
                    for s in &sessions {
                        s.tile_frame().remove_css_class("drop-hover");
                        s.card_frame().remove_css_class("drop-hover");
                    }
                    ws.clear_all_previews();
                }
                SessionEvent::DragHoverEnter(source_id, tx, ty) => {
                    // Skip if this event is a side-effect of a preview rebuild.
                    let guard = ws.inner.borrow().suppressing_hover.clone();
                    if !guard.get() {
                        ws.handle_drag_hover_enter(source_id, id, tx, ty);
                    }
                }
                SessionEvent::DragHoverMotion(source_id, tx, ty) => {
                    let guard = ws.inner.borrow().suppressing_hover.clone();
                    if !guard.get() {
                        ws.handle_drag_hover_motion(source_id, id, tx, ty);
                    }
                }
                SessionEvent::DragHoverLeave(source_id) => {
                    let guard = ws.inner.borrow().suppressing_hover.clone();
                    if !guard.get() {
                        ws.handle_drag_hover_leave(source_id, id);
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
        self.promote_at(id, None);
    }

    /// Promote a sidebar session into the arena at a specific slot index.
    /// If `slot` is None, appends to the end.
    pub fn promote_at(&self, id: u32, slot: Option<usize>) {
        let inner = self.inner.borrow();
        if inner.arena.contains(id) {
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
            match slot {
                Some(idx) => arena.insert_at(idx, incoming.clone()),
                None => { arena.add(incoming.clone()); }
            }
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

    /// Dispatch a drag-and-drop landing on a session target (`target_id`) from
    /// a dragged session (`source_id`). Branches on the region each id lives in
    /// and whether the arena has room; see the verification table in the plan.
    pub fn handle_drop(&self, source_id: u32, target_id: u32) {
        if source_id == target_id {
            return;
        }
        // Clear visual previews before executing the drop.
        self.clear_all_previews();
        let (arena, sidebar) = {
            let inner = self.inner.borrow();
            (inner.arena.clone(), inner.sidebar.clone())
        };

        let source_in_arena = arena.contains(source_id);
        let source_in_sidebar = sidebar.contains(source_id);
        let target_in_arena = arena.contains(target_id);
        let target_in_sidebar = sidebar.contains(target_id);

        match (source_in_arena, target_in_arena, source_in_sidebar, target_in_sidebar) {
            // Arena → arena reorder.
            (true, true, _, _) => arena.swap_sessions(source_id, target_id),

            // Sidebar → arena.
            (_, true, true, _) => {
                if !arena.is_full() {
                    // Room available: insert at the target's position.
                    let target_idx = arena.session_ids().iter().position(|&x| x == target_id);
                    let slot = target_idx.unwrap_or(arena.count());
                    self.promote_at(source_id, Some(slot));
                } else {
                    // Arena full: cross-region swap at the target tile's slot.
                    let Some(dragged) = sidebar.remove(source_id) else { return };
                    let Some(evicted) = arena.swap_at(target_id, dragged.clone()) else {
                        sidebar.add(dragged);
                        return;
                    };
                    evicted.place_in_sidebar();
                    sidebar.add(evicted);
                    dragged.place_in_arena();
                    let promoted_id = dragged.id();
                    let weak = Rc::downgrade(&self.inner);
                    glib::idle_add_local_once(move || {
                        if let Some(inner_rc) = weak.upgrade() {
                            Workspace { inner: inner_rc }.focus_session(promoted_id);
                        }
                    });
                }
            }

            // Arena → sidebar (drop on a specific card): cross-region swap.
            (true, _, _, true) => {
                let Some(target) = sidebar.remove(target_id) else { return };
                let Some(evicted) = arena.swap_at(source_id, target.clone()) else {
                    sidebar.add(target);
                    return;
                };
                evicted.place_in_sidebar();
                sidebar.add(evicted);
                target.place_in_arena();
                let promoted_id = target.id();
                let weak = Rc::downgrade(&self.inner);
                glib::idle_add_local_once(move || {
                    if let Some(inner_rc) = weak.upgrade() {
                        Workspace { inner: inner_rc }.focus_session(promoted_id);
                    }
                });
            }

            // Sidebar → sidebar: reorder cards.
            (_, _, true, true) => {
                sidebar.reorder_before(source_id, target_id);
            }

            _ => {}
        }
    }

    pub fn demote(&self, id: u32) {
        // Capture the arena position before removal so focus can shift to
        // the previous arena neighbour after the demote.
        let arena_idx = {
            let inner = self.inner.borrow();
            inner.arena.session_ids().iter().position(|x| *x == id)
        };

        let (arena, sidebar) = {
            let inner = self.inner.borrow();
            (inner.arena.clone(), inner.sidebar.clone())
        };

        let Some(s) = arena.remove(id) else {
            return;
        };
        sidebar.add(s.clone());
        s.place_in_sidebar();

        // Defer the focus shift: reparenting the VTE triggers GTK focus
        // bookkeeping that runs after this function returns, and would
        // override an immediate focus_session() call.
        if let Some(i) = arena_idx {
            let new_idx = i.saturating_sub(1);
            let weak = Rc::downgrade(&self.inner);
            glib::idle_add_local_once(move || {
                let Some(inner_rc) = weak.upgrade() else {
                    return;
                };
                let ws = Workspace { inner: inner_rc };
                let next = ws
                    .inner
                    .borrow()
                    .arena
                    .session_by_index(new_idx)
                    .map(|s| s.id());
                match next {
                    Some(nid) => ws.focus_session(nid),
                    None => {
                        ws.inner.borrow_mut().last_focused_id = None;
                        let sessions: Vec<Session> = ws.inner.borrow().registry.clone();
                        for s in &sessions {
                            s.set_focused(false);
                        }
                    }
                }
            });
        }
    }

    /// Demote the currently focused arena session to the sidebar. No-op if
    /// focus is in the sidebar or no session is focused in the arena.
    pub fn demote_focused(&self) {
        let id = self.inner.borrow().arena.focused().map(|s| s.id());
        if let Some(id) = id {
            self.demote(id);
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

    /// Focus the next arena session in display order, wrapping around.
    pub fn focus_next(&self) {
        let (ids, current) = {
            let inner = self.inner.borrow();
            (inner.arena.session_ids(), inner.arena.focused().map(|s| s.id()))
        };
        if ids.is_empty() {
            return;
        }
        let next = match current.and_then(|id| ids.iter().position(|x| *x == id)) {
            Some(i) => ids[(i + 1) % ids.len()],
            None => ids[0],
        };
        self.focus_session(next);
    }

    /// Focus the previous arena session in display order, wrapping around.
    pub fn focus_prev(&self) {
        let (ids, current) = {
            let inner = self.inner.borrow();
            (inner.arena.session_ids(), inner.arena.focused().map(|s| s.id()))
        };
        if ids.is_empty() {
            return;
        }
        let len = ids.len();
        let prev = match current.and_then(|id| ids.iter().position(|x| *x == id)) {
            Some(i) => ids[(i + len - 1) % len],
            None => ids[len - 1],
        };
        self.focus_session(prev);
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

    pub fn toggle_split(&self) {
        self.inner.borrow().arena.toggle_split();
    }

    pub fn new_session(&self) {
        let name = format!("Shell {}", self.inner.borrow().next_session_id);
        let home = std::env::var("HOME").ok();
        let session = self.spawn_session(&name, home.as_deref(), true);
        session.grab_focus();
    }

    /// Spawn a new session with its cwd copied from an existing one.
    pub fn clone_session(&self, id: u32) {
        let cwd = self.find(id).and_then(|s| s.current_dir());
        let fallback = std::env::var("HOME").ok();
        let dir = cwd.as_deref().or(fallback.as_deref());
        let name = format!("Shell {}", self.inner.borrow().next_session_id);
        let session = self.spawn_session(&name, dir, true);
        session.grab_focus();
    }

    /// Tear down a session: remove from arena/sidebar and drop from registry.
    pub fn close_session(&self, id: u32) {
        // Capture the arena position (if any) before removing, so we can
        // focus the neighbour immediately to the left.
        let arena_idx = {
            let inner = self.inner.borrow();
            if inner.arena.contains(id) {
                inner.arena.session_ids().iter().position(|x| *x == id)
            } else {
                None
            }
        };

        let (arena, sidebar) = {
            let inner = self.inner.borrow();
            (inner.arena.clone(), inner.sidebar.clone())
        };
        if arena_idx.is_some() {
            arena.remove(id);
        } else {
            sidebar.remove(id);
        }
        self.inner.borrow_mut().registry.retain(|s| s.id() != id);

        if let Some(i) = arena_idx {
            // Closed an arena session: focus the previous arena position. If
            // the closed one was the first, focus whatever is now at index 0
            // (i.e. what used to be at index 1). Drop focus if arena is empty.
            let new_idx = i.saturating_sub(1);
            let next = self
                .inner
                .borrow()
                .arena
                .session_by_index(new_idx)
                .map(|s| s.id());
            match next {
                Some(nid) => self.focus_session(nid),
                None => {
                    self.inner.borrow_mut().last_focused_id = None;
                    let sessions: Vec<Session> = self.inner.borrow().registry.clone();
                    for s in &sessions {
                        s.set_focused(false);
                    }
                }
            }
        } else if self.inner.borrow().last_focused_id == Some(id) {
            // Closed a sidebar session that happened to be last-focused; clear
            // the stale id but leave current arena focus untouched.
            self.inner.borrow_mut().last_focused_id = None;
        }
    }

    pub fn sessions(&self) -> Vec<Session> {
        self.inner.borrow().registry.clone()
    }

    /// Handle drag hover entering a session's drop zone.
    /// Uses tile-local coordinates translated to grid space for accurate
    /// phantom slot calculation.
    fn handle_drag_hover_enter(&self, source_id: u32, target_id: u32, tile_x: f64, tile_y: f64) {
        let (arena, sidebar, guard) = {
            let inner = self.inner.borrow();
            (inner.arena.clone(), inner.sidebar.clone(), inner.suppressing_hover.clone())
        };

        // Advance generation so any pending leave-clear timeouts become stale.
        let gen_rc = self.inner.borrow().drag_hover_gen.clone();
        gen_rc.set(gen_rc.get().wrapping_add(1));
        let gen = gen_rc.get();

        let source_in_sidebar = sidebar.contains(source_id);
        let target_in_arena = arena.contains(target_id);

        // Sidebar → arena (has room): show phantom at the cursor's grid position.
        if source_in_sidebar && target_in_arena && !arena.is_full() {
            let slot = self.grid_slot_from_tile(target_id, tile_x, tile_y);
            glib::idle_add_local_once(move || {
                if gen_rc.get() != gen { return; }
                guard.set(true);
                arena.ensure_phantom_at(slot);
                // Defer guard unset so post-rebuild events are suppressed.
                glib::idle_add_local_once(move || {
                    guard.set(false);
                });
            });
        }
    }

    /// Handle continuous cursor motion within a tile during drag.
    /// Updates phantom position based on actual cursor location in the grid.
    fn handle_drag_hover_motion(&self, source_id: u32, target_id: u32, tile_x: f64, tile_y: f64) {
        let (arena, sidebar, guard) = {
            let inner = self.inner.borrow();
            (inner.arena.clone(), inner.sidebar.clone(), inner.suppressing_hover.clone())
        };

        let source_in_sidebar = sidebar.contains(source_id);
        let target_in_arena = arena.contains(target_id);

        if source_in_sidebar && target_in_arena && !arena.is_full() {
            let slot = self.grid_slot_from_tile(target_id, tile_x, tile_y);
            if slot == arena.phantom_slot() && arena.has_phantom() {
                return; // Already at correct position.
            }
            let gen_rc = self.inner.borrow().drag_hover_gen.clone();
            let gen = gen_rc.get();
            glib::idle_add_local_once(move || {
                if gen_rc.get() != gen { return; }
                guard.set(true);
                arena.ensure_phantom_at(slot);
                glib::idle_add_local_once(move || {
                    guard.set(false);
                });
            });
        }
    }

    /// Translate tile-local coordinates to arena grid coordinates and compute
    /// the phantom slot index.
    fn grid_slot_from_tile(&self, target_id: u32, tile_x: f64, tile_y: f64) -> usize {
        let arena = self.inner.borrow().arena.clone();
        if let Some(target) = self.find(target_id) {
            let tile_frame = target.tile_frame();
            let grid = arena.widget();
            if let Some(p) = tile_frame.compute_point(
                &grid,
                &gtk::graphene::Point::new(tile_x as f32, tile_y as f32),
            ) {
                return arena.slot_from_coords(p.x() as f64, p.y() as f64);
            }
        }
        0
    }

    /// Handle drag hover leaving a session's drop zone.
    /// Phantom cleanup is handled by the grid leave timeout and DragEnded;
    /// per-tile leave only removes CSS classes (done in session.rs).
    fn handle_drag_hover_leave(&self, _source_id: u32, _target_id: u32) {
        // No-op: don't clear phantom here to avoid fighting with enter/motion
        // handlers after rebuild shifts tile positions.
    }

    /// Reset all drag-related visual previews to their normal state.
    fn clear_all_previews(&self) {
        let inner = self.inner.borrow();
        inner.arena.hide_phantom();
        inner.sidebar.hide_placeholder();
    }
}

