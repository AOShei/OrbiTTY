use crate::session::Session;

use super::Workspace;

impl Workspace {
    /// Open the peek popover on the most-relevant sidebar card: prefer the
    /// first card with raised attention, else the first sidebar card. No-op
    /// if the sidebar is empty.
    pub fn peek_best(&self) {
        let target = {
            let inner = self.inner.borrow();
            let sidebar_ids = inner.sidebar.session_ids();
            let by_id = |id: u32| inner.registry.iter().find(|s| s.id() == id).cloned();
            sidebar_ids
                .iter()
                .filter_map(|id| by_id(*id))
                .find(|s| s.has_attention())
                .or_else(|| sidebar_ids.first().and_then(|id| by_id(*id)))
        };
        if let Some(s) = target {
            s.peek();
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

    /// Make the last-focused session the sole occupant of the arena.
    /// All other arena sessions are silently demoted; if the target is in the
    /// sidebar it is promoted first. No-op if nothing is focused.
    pub fn solo_focused(&self) {
        let id = match self.inner.borrow().last_focused_id {
            Some(id) => id,
            None => return,
        };

        // Silently demote every arena session that isn't the target.
        let to_demote: Vec<u32> = self
            .inner
            .borrow()
            .arena
            .session_ids()
            .into_iter()
            .filter(|&x| x != id)
            .collect();

        for evict_id in to_demote {
            let (arena, sidebar) = {
                let inner = self.inner.borrow();
                (inner.arena.clone(), inner.sidebar.clone())
            };
            if let Some(s) = arena.remove(evict_id) {
                sidebar.add(s.clone());
                s.place_in_sidebar();
            }
        }

        // Promote target from sidebar if it isn't already in the arena.
        if !self.inner.borrow().arena.contains(id) {
            let (arena, sidebar) = {
                let inner = self.inner.borrow();
                (inner.arena.clone(), inner.sidebar.clone())
            };
            if let Some(s) = sidebar.remove(id) {
                arena.add(s.clone());
                s.place_in_arena();
            }
        }

        self.focus_session(id);
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

        // Notify the file tree of the newly focused session's CWD.
        // Use /proc as primary source (works for all shells + elevated sessions);
        // fall back to OSC 7 only if proc read fails.
        let cwd_cb = self.inner.borrow().cwd_change_cb.clone();
        if let Some(cb) = cwd_cb {
            let cwd = sessions
                .iter()
                .find(|s| s.id() == id)
                .and_then(|s| s.current_dir_proc().or_else(|| s.current_dir()));
            (cb)(cwd);
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
}
