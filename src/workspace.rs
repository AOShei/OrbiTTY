use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use crate::arena::{Arena, MAX_ACTIVE};
use crate::session::{Session, SessionCallback, SessionEvent};
use crate::sidebar::Sidebar;
use crate::templates::WorkspaceTemplate;

mod dnd;
mod focus;

/// Pool of animal emojis used to give each session a unique visual identity
/// within a workspace. Exhausted only if a workspace holds more sessions than
/// the pool size; duplicates are allowed past that point.
static EMOJI_POOL: &[&str] = &[
    "🦊", "🐼", "🐙", "🦉", "🐢", "🦄", "🐝", "🦒", "🐧", "🦋",
    "🦁", "🐯", "🐨", "🐸", "🐵", "🐰", "🐺", "🐻", "🐹", "🐭",
    "🦝", "🦔", "🦌", "🐷", "🐮", "🦙", "🦧", "🦥", "🦦", "🦨",
    "🦩", "🦢", "🦜", "🦚", "🐬", "🐳", "🦈", "🦑", "🐡", "🐌",
    "🦎", "🐍", "🦖", "🦕", "🦀",
];

pub struct WorkspaceInner {
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
    /// Called with Some(path) when the focused session's CWD changes, None when
    /// there is no focused session (e.g. after a close).
    pub cwd_change_cb: Option<Rc<dyn Fn(Option<String>)>>,
}

#[derive(Clone)]
pub struct Workspace {
    pub inner: Rc<RefCell<WorkspaceInner>>,
}

impl Workspace {
    pub fn new(template: &WorkspaceTemplate) -> Self {
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
            root,
            arena,
            sidebar,
            registry: Vec::new(),
            next_session_id: 1,
            last_focused_id: None,
            suppressing_hover: Rc::new(Cell::new(false)),
            drag_hover_gen: Rc::new(Cell::new(0)),
            cwd_change_cb: None,
        }));

        let workspace = Workspace { inner };

        workspace.install_background_drop_targets();

        // Build sessions from the template.
        for spec in &template.sessions {
            workspace.spawn_session(&spec.name, None, spec.promote);
        }

        // Focus first arena session if any.
        let first_id = workspace.inner.borrow().arena.session_by_index(0).map(|s| s.id());
        if let Some(id) = first_id {
            workspace.focus_session(id);
        }

        workspace.start_polling();

        workspace
    }

    pub fn widget(&self) -> gtk::Paned {
        self.inner.borrow().root.clone()
    }

    /// Create a new session, auto-promote if requested and arena has space.
    pub fn spawn_session(&self, name: &str, cwd: Option<&str>, promote: bool) -> Session {
        let sid = {
            let mut inner = self.inner.borrow_mut();
            let id = inner.next_session_id;
            inner.next_session_id += 1;
            id
        };

        let emoji = self.allocate_emoji();
        let cb = self.make_session_callback();
        let session = Session::new(sid, name, &emoji, cwd, cb);

        self.inner.borrow_mut().registry.push(session.clone());

        // Forward VTE CWD changes to the file tree when this session is focused.
        {
            let weak = Rc::downgrade(&self.inner);
            let sid = session.id();
            session.set_cwd_changed_cb(Box::new(move |path| {
                let Some(inner_rc) = weak.upgrade() else { return };
                let inner = inner_rc.borrow();
                if inner.last_focused_id == Some(sid) {
                    if let Some(cb) = &inner.cwd_change_cb {
                        (cb)(Some(path));
                    }
                }
            }));
        }

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
                SessionEvent::RequestClose => ws.request_close(id),
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
                    let sidebar = ws.inner.borrow().sidebar.clone();
                    if sidebar.contains(id) {
                        sidebar.set_dragging_id(id);
                    }
                }
                SessionEvent::DragEnded => {
                    ws.inner.borrow().root.remove_css_class("dragging");
                    ws.inner.borrow().sidebar.clear_dragging_id();
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

    /// Pick a random animal emoji not currently assigned to any session in
    /// this workspace. Falls back to a random pool member if every emoji is
    /// already in use.
    fn allocate_emoji(&self) -> String {
        let used: HashSet<String> = self
            .inner
            .borrow()
            .registry
            .iter()
            .map(|s| s.emoji())
            .collect();
        let available: Vec<&str> = EMOJI_POOL
            .iter()
            .copied()
            .filter(|e| !used.contains(*e))
            .collect();
        let pool: &[&str] = if available.is_empty() {
            EMOJI_POOL
        } else {
            &available
        };
        let idx = glib::random_int_range(0, pool.len() as i32) as usize;
        pool[idx].to_string()
    }

    /// Register a callback that fires whenever the currently focused session's
    /// CWD changes. Fires immediately with the current focused session's CWD.
    pub fn set_cwd_change_cb(&self, cb: impl Fn(Option<String>) + 'static) {
        let cb_rc: Rc<dyn Fn(Option<String>)> = Rc::new(cb);
        let current_cwd = {
            let inner = self.inner.borrow();
            inner
                .last_focused_id
                .and_then(|id| inner.registry.iter().find(|s| s.id() == id).cloned())
                .and_then(|s| s.current_dir())
        };
        self.inner.borrow_mut().cwd_change_cb = Some(cb_rc.clone());
        (cb_rc)(current_cwd);
    }

    pub fn find(&self, id: u32) -> Option<Session> {
        self.inner
            .borrow()
            .registry
            .iter()
            .find(|s| s.id() == id)
            .cloned()
    }

    pub fn focused_session(&self) -> Option<Session> {
        let inner = self.inner.borrow();
        let id = inner.last_focused_id?;
        inner.registry.iter().find(|s| s.id() == id).cloned()
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

    /// Entry point for close-button clicks. If the session has an active
    /// foreground process, prompt for confirmation before tearing it down;
    /// otherwise close immediately.
    pub fn request_close(&self, id: u32) {
        let Some(session) = self.find(id) else { return };
        if !session.is_busy() {
            self.close_session(id);
            return;
        }

        let emoji = session.emoji();
        let cmdline = session.foreground_process_cmdline();
        let body = format!(
            "A process is still running in Shell {}. Closing the terminal will terminate it.",
            emoji
        );
        let dialog = adw::AlertDialog::new(Some("Close this terminal?"), Some(&body));
        dialog.add_response("cancel", "_Cancel");
        dialog.add_response("close", "_Close Terminal");
        dialog.set_response_appearance("close", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        // prefer-wide-layout is libadwaita >= 1.6; the typed setter is gated
        // behind the v1_6 feature, so set it dynamically. libadwaita gracefully
        // ignores unknown properties on older runtimes.
        dialog.set_property("prefer-wide-layout", true);
        dialog.set_content_width(460);

        // GNOME Console-style command bubble between the body and the buttons.
        if let Some(cmd) = cmdline {
            let cmd_label = gtk::Label::new(Some(&cmd));
            cmd_label.set_xalign(0.0);
            cmd_label.set_halign(gtk::Align::Fill);
            cmd_label.set_hexpand(true);
            cmd_label.set_wrap(true);
            cmd_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            // Deliberately not selectable: a selectable label grabs focus
            // from the default "Cancel" response and renders with all text
            // highlighted on open. Read-only display is enough here.
            cmd_label.add_css_class("monospace");
            cmd_label.add_css_class("orbit-close-dialog-command");
            dialog.set_extra_child(Some(&cmd_label));
        }

        let ws = self.clone();
        dialog.connect_response(None, move |_dlg, response| {
            if response == "close" {
                ws.close_session(id);
            }
        });

        let parent = self.inner.borrow().root.clone();
        dialog.present(Some(&parent));
    }

    /// Tear down a session: remove from arena/sidebar and drop from registry.
    pub fn close_session(&self, id: u32) {
        let (arena_idx, arena, sidebar) = {
            let inner = self.inner.borrow();
            // Short-circuit if the session was already torn down (e.g. a
            // deferred child-exited callback racing a manual close).
            if !inner.registry.iter().any(|s| s.id() == id) {
                return;
            }
            let idx = if inner.arena.contains(id) {
                inner.arena.session_ids().iter().position(|x| *x == id)
            } else {
                None
            };
            (idx, inner.arena.clone(), inner.sidebar.clone())
        };

        if arena_idx.is_some() {
            arena.remove(id);
        } else {
            sidebar.remove(id);
        }

        // Pop the Session out first, then let it drop *after* the borrow is
        // released. Dropping can synchronously emit signals (VTE child-exited)
        // that re-enter Workspace; holding a borrow across that is a panic.
        let removed = {
            let mut inner = self.inner.borrow_mut();
            inner.registry
                .iter()
                .position(|s| s.id() == id)
                .map(|p| inner.registry.remove(p))
        };
        drop(removed);

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

    fn start_polling(&self) {
        let weak = Rc::downgrade(&self.inner);
        glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
            let Some(inner_rc) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let (sessions, sidebar) = {
                let inner = inner_rc.borrow();
                (inner.registry.clone(), inner.sidebar.clone())
            };
            for session in &sessions {
                session.tick();
            }
            sidebar.tick();

            // /proc/[pid]/cwd fallback: detect CWD changes for shells that do
            // not emit OSC 7.  Only fires when the value actually changes.
            let (changed_cwd, cwd_cb) = {
                let inner = inner_rc.borrow();
                let changed = inner
                    .last_focused_id
                    .and_then(|id| inner.registry.iter().find(|s| s.id() == id).cloned())
                    .and_then(|s| s.poll_cwd_changed());
                (changed, inner.cwd_change_cb.clone())
            };
            if let (Some(path), Some(cb)) = (changed_cwd, cwd_cb) {
                (cb)(Some(path));
            }

            glib::ControlFlow::Continue
        });
    }
}
