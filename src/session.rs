use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4 as gtk;
use libadwaita as adw;
use vte4::prelude::*;

mod runtime;
mod view;

use self::runtime::{
    cwd_tracking_pid, foreground_command, format_duration_compact, is_foreground_elevated,
    is_terminal_idle,
};
use self::view::{
    apply_vte_theme, build_session_widgets, configure_terminal, set_vte_interactive,
    sync_elevated_headers, unparent_from_box,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    Arena,
    Sidebar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipState {
    Idle,
    Busy,
    Alert,
}

impl PipState {
    fn css(self) -> &'static str {
        match self {
            PipState::Idle => "idle",
            PipState::Busy => "busy",
            PipState::Alert => "alert",
        }
    }
}

/// Callback dispatcher for a session.
pub type SessionCallback = Rc<RefCell<Box<dyn Fn(u32, SessionEvent) + 'static>>>;

#[derive(Debug, Clone, Copy)]
pub enum SessionEvent {
    RequestPromote,
    RequestDemote,
    RequestClose,
    RequestClone,
    RequestSwap(u32),
    Focused,
    Bell,
    DragStarted,
    DragEnded,
    /// A drag carrying `source_id` entered this session's drop zone (tile-local coords).
    DragHoverEnter(u32, f64, f64),
    /// Cursor moved within this session's tile while dragging (tile-local coords).
    DragHoverMotion(u32, f64, f64),
    /// A drag left this session's drop zone.
    DragHoverLeave(u32),
}

pub struct SessionInner {
    pub id: u32,
    pub emoji: String,
    pub vte: vte4::Terminal,

    // Arena tile (outer frame + header + content slot).
    pub tile_frame: gtk::Box,
    pub tile_header: gtk::Box,
    pub tile_slot: gtk::Box,
    pub tile_title: gtk::Label,
    pub tile_pip: gtk::Box,

    // Sidebar card.
    pub card_frame: gtk::Box,
    pub card_header: gtk::Box,
    pub card_slot: gtk::Box,
    pub card_title: gtk::Label,
    pub card_pip: gtk::Box,
    /// Metrics caption ("idle 15s", "busy 2m").
    pub metrics_label: gtk::Label,

    pub location: Location,
    pub pip_state: PipState,
    pub elevated: bool,
    pub focused: bool,
    pub is_busy: bool,
    /// When the current busy/idle state began. Drives the metrics caption.
    pub state_since: Instant,
    /// Raised when a demoted session transitions Busy→Idle, meaning the user
    /// should look — the process likely finished or is waiting for input.
    /// Cleared on promote, peek, or explicit dismiss.
    pub attention: bool,
    pub shell_pid: Option<i32>,
    pub alert_until: Option<Instant>,
    /// Active peek popover, if the VTE is currently reparented into one.
    pub peek_popover: Option<gtk::Popover>,
    /// Called with the new path whenever OSC 7 reports a CWD change.
    pub cwd_changed_cb: Option<Box<dyn Fn(String)>>,
    /// Last CWD seen by either OSC 7 or /proc polling; used to suppress
    /// redundant fire-and-re-fire on the same path.
    pub last_known_cwd: Option<String>,
    /// Text fed to the PTY immediately after the shell process spawns.
    /// Used to send commands (e.g. sudo) before the first prompt appears.
    pub post_spawn_text: Option<String>,
}

#[derive(Clone)]
pub struct Session {
    pub inner: Rc<RefCell<SessionInner>>,
}

impl Session {
    pub fn new(id: u32, name: &str, emoji: &str, cwd: Option<&str>, cb: SessionCallback) -> Self {
        let vte = vte4::Terminal::builder()
            .scrollback_lines(10_000)
            .hexpand(true)
            .vexpand(true)
            .build();
        configure_terminal(&vte);

        let widgets = build_session_widgets(name, emoji);

        // Clicking the header focuses this tile's VTE.
        {
            let cb = cb.clone();
            let gesture = gtk::GestureClick::new();
            gesture.set_button(gtk::gdk::BUTTON_PRIMARY);
            gesture.connect_pressed(move |_, _, _, _| {
                (cb.borrow())(id, SessionEvent::Focused);
            });
            widgets.tile_header.add_controller(gesture);
        }

        // Drag source on the tile header for arena reordering.
        {
            let cb = cb.clone();
            let drag_source = gtk::DragSource::new();
            drag_source.set_actions(gtk::gdk::DragAction::MOVE);
            drag_source.connect_prepare(move |_src, _x, _y| {
                Some(gtk::gdk::ContentProvider::for_value(&id.to_value()))
            });
            {
                let cb = cb.clone();
                drag_source.connect_drag_begin(move |_src, _drag| {
                    (cb.borrow())(id, SessionEvent::DragStarted);
                });
            }
            {
                let cb = cb.clone();
                drag_source.connect_drag_end(move |_src, _drag, _delete| {
                    (cb.borrow())(id, SessionEvent::DragEnded);
                });
            }
            widgets.tile_header.add_controller(drag_source);
        }

        // Drag source on the card header so cards can be dragged into the arena.
        {
            let cb = cb.clone();
            let drag_source = gtk::DragSource::new();
            drag_source.set_actions(gtk::gdk::DragAction::MOVE);
            drag_source.connect_prepare(move |_src, _x, _y| {
                Some(gtk::gdk::ContentProvider::for_value(&id.to_value()))
            });
            {
                let cb = cb.clone();
                drag_source.connect_drag_begin(move |_src, _drag| {
                    (cb.borrow())(id, SessionEvent::DragStarted);
                });
            }
            {
                let cb = cb.clone();
                drag_source.connect_drag_end(move |_src, _drag, _delete| {
                    (cb.borrow())(id, SessionEvent::DragEnded);
                });
            }
            widgets.card_header.add_controller(drag_source);
        }

        let inner = Rc::new(RefCell::new(SessionInner {
            id,
            emoji: emoji.to_string(),
            vte: vte.clone(),
            tile_frame: widgets.tile_frame.clone(),
            tile_header: widgets.tile_header.clone(),
            tile_slot: widgets.tile_slot.clone(),
            tile_title: widgets.tile_title.clone(),
            tile_pip: widgets.tile_pip.clone(),
            card_frame: widgets.card_frame.clone(),
            card_header: widgets.card_header.clone(),
            card_slot: widgets.card_slot.clone(),
            card_title: widgets.card_title.clone(),
            card_pip: widgets.card_pip.clone(),
            metrics_label: widgets.metrics_label.clone(),
            location: Location::Sidebar, // will be placed by workspace
            pip_state: PipState::Idle,
            elevated: false,
            focused: false,
            is_busy: false,
            state_since: Instant::now(),
            attention: false,
            shell_pid: None,
            alert_until: None,
            peek_popover: None,
            cwd_changed_cb: None,
            last_known_cwd: None,
            post_spawn_text: None,
        }));

        let session = Session { inner };

        // Update VTE colors + elevated header tint when theme changes.
        {
            let vte_weak = vte.downgrade();
            let inner_weak = Rc::downgrade(&session.inner);
            adw::StyleManager::default().connect_dark_notify(move |_sm| {
                if let Some(vte) = vte_weak.upgrade() {
                    apply_vte_theme(&vte);
                }
                if let Some(inner_rc) = inner_weak.upgrade() {
                    Session { inner: inner_rc }.refresh_elevated_theme();
                }
            });
        }

        // Wire events.
        {
            let cb = cb.clone();
            widgets.promote_btn.connect_clicked(move |_| {
                (cb.borrow())(id, SessionEvent::RequestPromote);
            });
        }
        {
            let cb = cb.clone();
            widgets.demote_btn.connect_clicked(move |_| {
                (cb.borrow())(id, SessionEvent::RequestDemote);
            });
        }
        {
            let cb = cb.clone();
            widgets.tile_clone_btn.connect_clicked(move |_| {
                (cb.borrow())(id, SessionEvent::RequestClone);
            });
        }
        {
            let cb = cb.clone();
            widgets.tile_close_btn.connect_clicked(move |_| {
                (cb.borrow())(id, SessionEvent::RequestClose);
            });
        }
        {
            let cb = cb.clone();
            widgets.card_close_btn.connect_clicked(move |_| {
                (cb.borrow())(id, SessionEvent::RequestClose);
            });
        }
        // Click on the card body (preview area) opens the peek popover.
        // The header keeps its own drag-source / focus / button behavior.
        // Capture phase + claim preempts the live-preview VTE's own gestures
        // (selection, focus) so the entire card reads as a clickable surface.
        {
            let session_weak = Rc::downgrade(&session.inner);
            let card_slot = session.inner.borrow().card_slot.clone();
            let gesture = gtk::GestureClick::new();
            gesture.set_button(gtk::gdk::BUTTON_PRIMARY);
            gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
            gesture.connect_pressed(move |g, _, _, _| {
                g.set_state(gtk::EventSequenceState::Claimed);
            });
            gesture.connect_released(move |_, _, _, _| {
                if let Some(inner_rc) = session_weak.upgrade() {
                    Session { inner: inner_rc }.peek();
                }
            });
            card_slot.add_controller(gesture);
        }
        {
            // Focus tile on VTE focus.
            let cb = cb.clone();
            let ctl = gtk::EventControllerFocus::new();
            ctl.connect_enter(move |_| {
                (cb.borrow())(id, SessionEvent::Focused);
            });
            vte.add_controller(ctl);
        }
        {
            // Ctrl+Shift+C / Ctrl+Shift+V for copy/paste. VTE doesn't bind
            // these itself; intercept during the capture phase so the
            // terminal doesn't swallow them as input first.
            let vte_kb = vte.clone();
            let key_ctl = gtk::EventControllerKey::new();
            key_ctl.set_propagation_phase(gtk::PropagationPhase::Capture);
            key_ctl.connect_key_pressed(move |_, keyval, _keycode, state| {
                use gtk::gdk::ModifierType;
                let mods = state
                    & (ModifierType::CONTROL_MASK
                        | ModifierType::SHIFT_MASK
                        | ModifierType::ALT_MASK);
                if mods != (ModifierType::CONTROL_MASK | ModifierType::SHIFT_MASK) {
                    return glib::Propagation::Proceed;
                }
                match keyval.to_lower() {
                    gtk::gdk::Key::c => {
                        vte_kb.copy_clipboard_format(vte4::Format::Text);
                        glib::Propagation::Stop
                    }
                    gtk::gdk::Key::v => {
                        vte_kb.paste_clipboard();
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            });
            vte.add_controller(key_ctl);
        }
        {
            // Bell → alert pip.
            let cb = cb.clone();
            vte.connect_bell(move |_| {
                (cb.borrow())(id, SessionEvent::Bell);
            });
        }
        {
            // Shell exit (e.g. user typed `exit`) → close the session.
            // Deferred via idle_add so we never re-enter Workspace during
            // another operation (e.g. a VTE drop triggered by close_session
            // itself would synchronously fire this signal, causing a
            // RefCell reentrant-borrow panic).
            let cb = cb.clone();
            vte.connect_child_exited(move |_, _status| {
                let cb = cb.clone();
                glib::idle_add_local_once(move || {
                    (cb.borrow())(id, SessionEvent::RequestClose);
                });
            });
        }
        {
            // Track window title (user@host:path) set by the shell.
            let weak = Rc::downgrade(&session.inner);
            vte.connect_window_title_changed(move |vte| {
                if let Some(inner_rc) = weak.upgrade() {
                    let title = vte.window_title().unwrap_or_default();
                    let inner = inner_rc.borrow();
                    inner.tile_title.set_text(&title);
                    inner.card_title.set_text(&title);
                }
            });
        }
        {
            // Real-time CWD updates via OSC 7 (current-directory-uri).
            // Also syncs last_known_cwd so the /proc poll does not double-fire.
            let weak = Rc::downgrade(&session.inner);
            vte.connect_current_directory_uri_notify(move |vte| {
                let Some(inner_rc) = weak.upgrade() else { return };
                let uri = vte.current_directory_uri();
                let Some(uri) = uri else { return };
                let file = gio::File::for_uri(uri.as_str());
                let Some(path) = file.path().and_then(|p| p.to_str().map(|s| s.to_string()))
                else {
                    return;
                };
                // Sync so /proc poll won't re-fire for the same path 250ms later.
                inner_rc.borrow_mut().last_known_cwd = Some(path.clone());
                // Fire the workspace callback (separate borrow — callback borrows WorkspaceInner).
                let inner = inner_rc.borrow();
                if let Some(cb) = &inner.cwd_changed_cb {
                    (cb)(path);
                }
            });
        }
        // Drop target on tile for arena swap.
        {
            let cb = cb.clone();
            let tile_frame = session.inner.borrow().tile_frame.clone();
            let drop_target = gtk::DropTarget::new(
                <u32 as glib::types::StaticType>::static_type(),
                gtk::gdk::DragAction::MOVE,
            );
            {
                let cb = cb.clone();
                let tile_frame = tile_frame.clone();
                drop_target.connect_enter(move |dt, x, y| {
                    tile_frame.add_css_class("drop-hover");
                    if let Some(source_id) = extract_source_id(dt) {
                        if source_id != id {
                            (cb.borrow())(id, SessionEvent::DragHoverEnter(source_id, x, y));
                        }
                    }
                    gtk::gdk::DragAction::MOVE
                });
            }
            {
                let cb = cb.clone();
                drop_target.connect_motion(move |dt, x, y| {
                    if let Some(source_id) = extract_source_id(dt) {
                        if source_id != id {
                            (cb.borrow())(id, SessionEvent::DragHoverMotion(source_id, x, y));
                        }
                    }
                    gtk::gdk::DragAction::MOVE
                });
            }
            {
                let cb = cb.clone();
                let tile_frame = tile_frame.clone();
                drop_target.connect_leave(move |dt| {
                    tile_frame.remove_css_class("drop-hover");
                    if let Some(source_id) = extract_source_id(dt) {
                        if source_id != id {
                            (cb.borrow())(id, SessionEvent::DragHoverLeave(source_id));
                        }
                    }
                });
            }
            drop_target.connect_drop(move |_target, value, _x, _y| {
                if let Ok(source_id) = value.get::<u32>() {
                    if source_id != id {
                        (cb.borrow())(id, SessionEvent::RequestSwap(source_id));
                    }
                    return true;
                }
                false
            });
            session.inner.borrow().tile_frame.add_controller(drop_target);
        }
        // Drop target on the sidebar card for cross-region swap.
        // Only activates when this card is in the sidebar AND the source is
        // from the arena (cross-region). Sidebar→sidebar reorders are handled
        // by the list-level DropTarget which shows a placeholder.
        {
            let cb = cb.clone();
            let card_frame = session.inner.borrow().card_frame.clone();
            let inner_weak = Rc::downgrade(&session.inner);
            let drop_target = gtk::DropTarget::new(
                <u32 as glib::types::StaticType>::static_type(),
                gtk::gdk::DragAction::MOVE,
            );
            {
                let cb = cb.clone();
                let card_frame = card_frame.clone();
                let inner_weak = inner_weak.clone();
                drop_target.connect_enter(move |dt, _x, _y| {
                    // Reject if this card is in the sidebar and so is the source —
                    // let the list-level DropTarget handle sidebar reordering.
                    if let Some(inner_rc) = inner_weak.upgrade() {
                        let loc = inner_rc.borrow().location;
                        if loc == crate::session::Location::Sidebar {
                            if let Some(_source_id) = extract_source_id(dt) {
                                // We can't check the source's location easily here,
                                // so reject all drops on sidebar cards — the list
                                // DropTarget handles both sidebar→sidebar reorder
                                // and arena→sidebar demote.
                                return gtk::gdk::DragAction::empty();
                            }
                        }
                    }
                    card_frame.add_css_class("drop-hover");
                    if let Some(source_id) = extract_source_id(dt) {
                        if source_id != id {
                            (cb.borrow())(id, SessionEvent::DragHoverEnter(source_id, 0.0, 0.0));
                        }
                    }
                    gtk::gdk::DragAction::MOVE
                });
            }
            {
                let cb = cb.clone();
                let card_frame = card_frame.clone();
                drop_target.connect_leave(move |dt| {
                    card_frame.remove_css_class("drop-hover");
                    if let Some(source_id) = extract_source_id(dt) {
                        if source_id != id {
                            (cb.borrow())(id, SessionEvent::DragHoverLeave(source_id));
                        }
                    }
                });
            }
            drop_target.connect_drop(move |_target, value, _x, _y| {
                if let Ok(source_id) = value.get::<u32>() {
                    if source_id != id {
                        (cb.borrow())(id, SessionEvent::RequestSwap(source_id));
                    }
                    return true;
                }
                false
            });
            session.inner.borrow().card_frame.add_controller(drop_target);
        }
        // Spawn default shell. cwd defaults to $HOME when not supplied.
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let argv: [&str; 1] = [shell.as_str()];
        let envv: [&str; 0] = [];
        let home = std::env::var("HOME").ok();
        let working_dir = cwd.or(home.as_deref());
        {
            let weak = Rc::downgrade(&session.inner);
            vte.spawn_async(
                vte4::PtyFlags::DEFAULT,
                working_dir,
                &argv,
                &envv,
                glib::SpawnFlags::DEFAULT,
                || {},
                -1,
                None::<&gio::Cancellable>,
                move |result| {
                        if let Ok(pid) = result {
                            if let Some(inner_rc) = weak.upgrade() {
                                let post_text = {
                                    let mut inner = inner_rc.borrow_mut();
                                    inner.shell_pid = Some(pid.0 as i32);
                                    inner.post_spawn_text.take()
                                };
                                if let Some(text) = post_text {
                                    inner_rc.borrow().vte.feed_child(text.as_bytes());
                                }
                            }
                        }
                    },
                );
        }

        session
    }

    pub fn id(&self) -> u32 {
        self.inner.borrow().id
    }

    pub fn emoji(&self) -> String {
        self.inner.borrow().emoji.clone()
    }

    pub fn tile_frame(&self) -> gtk::Box {
        self.inner.borrow().tile_frame.clone()
    }

    pub fn card_frame(&self) -> gtk::Box {
        self.inner.borrow().card_frame.clone()
    }

    /// Move the VTE widget into the arena tile slot.
    ///
    /// Focus is intentionally NOT grabbed here — the caller (Workspace)
    /// does that explicitly, and doing it here while a borrow is held
    /// would re-enter via the focus-in signal and panic.
    pub fn place_in_arena(&self) {
        let (vte_widget, tile_slot) = {
            let inner = self.inner.borrow();
            (
                inner.vte.clone().upcast::<gtk::Widget>(),
                inner.tile_slot.clone(),
            )
        };
        unparent_from_box(&vte_widget);
        tile_slot.append(&vte_widget);
        set_vte_interactive(&self.inner.borrow().vte, true);
        self.inner.borrow_mut().location = Location::Arena;
        // Promotion means the user is now looking at the session directly;
        // any pending attention alert is implicitly acknowledged.
        self.set_attention(false);
    }

    pub fn place_in_sidebar(&self) {
        let (vte_widget, card_slot) = {
            let inner = self.inner.borrow();
            (
                inner.vte.clone().upcast::<gtk::Widget>(),
                inner.card_slot.clone(),
            )
        };
        unparent_from_box(&vte_widget);
        // Prepend so metrics_label stays as the final child below the VTE.
        card_slot.insert_child_after(&vte_widget, None::<&gtk::Widget>);
        set_vte_interactive(&self.inner.borrow().vte, false);
        self.inner.borrow_mut().location = Location::Sidebar;
        self.refresh_preview();
    }

    /// Refresh the metrics caption ("idle 15s", "busy 2m") based on current
    /// busy/idle state. The live VTE handles its own rendering.
    pub fn refresh_preview(&self) {
        let (metrics_label, is_busy, state_since) = {
            let inner = self.inner.borrow();
            (
                inner.metrics_label.clone(),
                inner.is_busy,
                inner.state_since,
            )
        };
        let state = if is_busy { "busy" } else { "idle" };
        let elapsed = Instant::now().saturating_duration_since(state_since);
        metrics_label.set_text(&format!("{} {}", state, format_duration_compact(elapsed)));
    }

    pub fn set_focused(&self, focused: bool) {
        let mut inner = self.inner.borrow_mut();
        inner.focused = focused;
        if focused {
            inner.tile_frame.add_css_class("focused");
            inner.card_frame.add_css_class("active");
        } else {
            inner.tile_frame.remove_css_class("focused");
            inner.card_frame.remove_css_class("active");
        }
    }

    pub fn is_focused(&self) -> bool {
        self.inner.borrow().focused
    }

    pub fn grab_focus(&self) {
        // Clone the Terminal out so the RefCell borrow drops *before* the
        // GTK call, which can synchronously emit focus-in and re-enter us.
        let vte = self.inner.borrow().vte.clone();
        vte.grab_focus();
    }

    pub fn is_busy(&self) -> bool {
        self.inner.borrow().is_busy
    }

    /// Full command line (e.g. "node /usr/bin/gemini") of the foreground
    /// process if the terminal is currently busy; None when the shell is at
    /// a prompt.
    pub fn foreground_process_cmdline(&self) -> Option<String> {
        let pid = self.inner.borrow().shell_pid?;
        foreground_command(pid)
    }

    pub fn has_attention(&self) -> bool {
        self.inner.borrow().attention
    }

    /// Toggle the attention state. Adds/removes the `.attention` CSS class
    /// on the card so the pulse animation starts/stops.
    pub fn set_attention(&self, on: bool) {
        let (changed, card_frame) = {
            let mut inner = self.inner.borrow_mut();
            if inner.attention == on {
                (false, inner.card_frame.clone())
            } else {
                inner.attention = on;
                (true, inner.card_frame.clone())
            }
        };
        if !changed {
            return;
        }
        if on {
            card_frame.add_css_class("attention");
        } else {
            card_frame.remove_css_class("attention");
        }
    }

    /// Open a peek popover over the sidebar card: reparent the VTE from the
    /// card's preview slot into the popover so the user can look at — and
    /// type into — the terminal without promoting it to the arena. On dismiss
    /// the VTE is reparented back into the card slot as the live read-only
    /// preview. Peeking implicitly acknowledges any pending attention.
    ///
    /// No-op if the session is not in the sidebar or already being peeked.
    pub fn peek(&self) {
        let (location, already_open) = {
            let inner = self.inner.borrow();
            (inner.location, inner.peek_popover.is_some())
        };
        if location != Location::Sidebar || already_open {
            return;
        }

        let (card_frame, card_slot, vte, vte_widget) = {
            let inner = self.inner.borrow();
            (
                inner.card_frame.clone(),
                inner.card_slot.clone(),
                inner.vte.clone(),
                inner.vte.clone().upcast::<gtk::Widget>(),
            )
        };

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.set_size_request(640, 400);
        content.add_css_class("orbit-peek-body");

        // Placeholder shown in the card slot while the VTE lives in the popover.
        let placeholder = gtk::Box::new(gtk::Orientation::Vertical, 0);
        placeholder.set_vexpand(true);
        placeholder.set_hexpand(true);
        placeholder.add_css_class("orbit-peek-placeholder");
        let ph_center = gtk::Box::new(gtk::Orientation::Vertical, 6);
        ph_center.set_valign(gtk::Align::Center);
        ph_center.set_halign(gtk::Align::Center);
        ph_center.set_vexpand(true);
        ph_center.set_opacity(0.35);
        let ph_icon = gtk::Image::from_icon_name("utilities-terminal-symbolic");
        ph_icon.set_pixel_size(20);
        let ph_label = gtk::Label::new(Some("ACTIVE_"));
        ph_label.add_css_class("orbit-peek-placeholder-label");
        ph_center.append(&ph_icon);
        ph_center.append(&ph_label);
        placeholder.append(&ph_center);

        unparent_from_box(&vte_widget);
        card_slot.insert_child_after(&placeholder, None::<&gtk::Widget>);
        content.append(&vte_widget);
        set_vte_interactive(&vte, true);

        let popover = gtk::Popover::new();
        popover.add_css_class("orbit-peek-popover");
        popover.set_parent(&card_frame);
        popover.set_position(gtk::PositionType::Left);
        popover.set_autohide(true);
        popover.set_has_arrow(true);
        popover.set_child(Some(&content));

        // Reparent VTE back into the card slot (as read-only preview) on
        // close, and drop the popover so another peek can open.
        {
            let session = self.clone();
            let placeholder = placeholder.clone();
            popover.connect_closed(move |pop| {
                let (card_slot, vte, vte_widget) = {
                    let inner = session.inner.borrow();
                    (
                        inner.card_slot.clone(),
                        inner.vte.clone(),
                        inner.vte.clone().upcast::<gtk::Widget>(),
                    )
                };
                unparent_from_box(&vte_widget);
                card_slot.remove(&placeholder);
                card_slot.insert_child_after(&vte_widget, None::<&gtk::Widget>);
                set_vte_interactive(&vte, false);
                session.inner.borrow_mut().peek_popover = None;
                session.refresh_preview();
                // Detach popover from its parent widget so the next peek
                // can create a fresh one without leaking GTK parent state.
                pop.unparent();
            });
        }

        // Stash reference and clear attention before showing.
        self.inner.borrow_mut().peek_popover = Some(popover.clone());
        // Peeking acknowledges the alert — no need for the pulse to continue.
        self.set_attention(false);

        popover.popup();
        // Let the user type immediately.
        vte.grab_focus();
    }

    pub fn set_pip(&self, state: PipState) {
        let mut inner = self.inner.borrow_mut();
        if inner.pip_state == state {
            return;
        }
        for cls in ["idle", "busy", "alert"] {
            inner.tile_pip.remove_css_class(cls);
            inner.card_pip.remove_css_class(cls);
        }
        inner.tile_pip.add_css_class(state.css());
        inner.card_pip.add_css_class(state.css());
        inner.pip_state = state;
    }

    fn set_elevated(&self, elevated: bool) {
        let mut inner = self.inner.borrow_mut();
        if inner.elevated == elevated {
            return;
        }
        inner.elevated = elevated;
        sync_elevated_headers(&inner.tile_header, &inner.card_header, elevated);
    }

    /// Swap elevated-dark ↔ elevated-light when the theme changes.
    fn refresh_elevated_theme(&self) {
        let inner = self.inner.borrow();
        if !inner.elevated {
            return;
        }
        sync_elevated_headers(&inner.tile_header, &inner.card_header, true);
    }

    pub fn raise_alert(&self) {
        self.set_pip(PipState::Alert);
        self.inner.borrow_mut().alert_until =
            Some(Instant::now() + Duration::from_millis(1500));
    }

    pub(crate) fn tick(&self) {
        let (pid, alert_until, prev_busy, location) = {
            let inner = self.inner.borrow();
            (inner.shell_pid, inner.alert_until, inner.is_busy, inner.location)
        };

        // Under an active alert we freeze the pip state but still let
        // the preview tick so long-running demoted sessions don't appear
        // stale on-screen.
        let alert_active = alert_until.map(|u| Instant::now() < u).unwrap_or(false);
        if !alert_active && alert_until.is_some() {
            self.inner.borrow_mut().alert_until = None;
        }

        if let Some(pid) = pid {
            let now_busy = !is_terminal_idle(pid);
            if !alert_active {
                self.set_pip(if now_busy { PipState::Busy } else { PipState::Idle });
            }
            if now_busy != prev_busy {
                {
                    let mut inner = self.inner.borrow_mut();
                    inner.is_busy = now_busy;
                    inner.state_since = Instant::now();
                }
                // Busy→Idle in the dock = "needs attention": the process
                // just finished or is waiting for input while off-stage.
                if !now_busy && location == Location::Sidebar {
                    self.set_attention(true);
                }
            }
            self.set_elevated(is_foreground_elevated(pid));
        }

        if location == Location::Sidebar {
            self.refresh_preview();
        }
    }

    pub fn set_cwd_changed_cb(&self, cb: Box<dyn Fn(String)>) {
        self.inner.borrow_mut().cwd_changed_cb = Some(cb);
    }

    /// Read CWD from /proc/[pid]/cwd — works for all shells, no OSC 7 needed.
    pub fn current_dir_proc(&self) -> Option<String> {
        let shell_pid = self.inner.borrow().shell_pid?;
        let pid = cwd_tracking_pid(shell_pid);
        let link = std::fs::read_link(format!("/proc/{}/cwd", pid)).ok()?;
        link.to_str().map(|s| s.to_string())
    }

    /// Compare /proc CWD against last known value. Returns the new path if it
    /// has changed (and updates the stored value); returns None if unchanged.
    pub fn poll_cwd_changed(&self) -> Option<String> {
        let proc_cwd = self.current_dir_proc()?;
        let mut inner = self.inner.borrow_mut();
        if inner.last_known_cwd.as_deref() == Some(proc_cwd.as_str()) {
            return None;
        }
        inner.last_known_cwd = Some(proc_cwd.clone());
        Some(proc_cwd)
    }

    /// Current working directory of the session's shell, derived from OSC 7
    /// (current-directory-uri). Returns None until the shell emits it.
    pub fn current_dir(&self) -> Option<String> {
        let uri = self.inner.borrow().vte.current_directory_uri()?;
        let file = gio::File::for_uri(uri.as_str());
        file.path().and_then(|p| p.to_str().map(|s| s.to_string()))
    }

    pub fn set_font_scale(&self, scale: f64) {
        let vte = self.inner.borrow().vte.clone();
        vte.set_font_scale(scale);
    }

    pub fn send_cd(&self, path: &str) {
        let safe = path.replace('\'', "'\\''");
        let cmd = format!("cd -- '{}'\n", safe);
        let vte = self.inner.borrow().vte.clone();
        vte.feed_child(cmd.as_bytes());
    }

    /// Queue text to be fed to the PTY as soon as the shell process spawns.
    /// If the shell is already running, sends immediately.
    pub fn set_post_spawn_text(&self, text: String) {
        let mut inner = self.inner.borrow_mut();
        if inner.shell_pid.is_some() {
            let vte = inner.vte.clone();
            drop(inner);
            vte.feed_child(text.as_bytes());
        } else {
            inner.post_spawn_text = Some(text);
        }
    }

}

/// Extract the source session id from a DropTarget's current drag.
pub(crate) fn extract_source_id(dt: &gtk::DropTarget) -> Option<u32> {
    let drop = dt.current_drop()?;
    let drag = drop.drag()?;
    let content = drag.content();
    content.value(<u32 as glib::types::StaticType>::static_type()).ok()?.get::<u32>().ok()
}
