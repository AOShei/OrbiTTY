use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4 as gtk;
use libadwaita as adw;
use vte4::prelude::*;

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
    pub name: String,
    pub emoji: String,
    pub vte: vte4::Terminal,

    // Arena tile (outer frame + header + content slot).
    pub tile_frame: gtk::Box,
    pub tile_header: gtk::Box,
    pub tile_slot: gtk::Box,
    pub tile_title: gtk::Label,
    pub tile_pip: gtk::Box,
    pub demote_btn: gtk::Button,
    pub tile_clone_btn: gtk::Button,
    pub tile_close_btn: gtk::Button,

    // Sidebar card.
    pub card_frame: gtk::Box,
    pub card_header: gtk::Box,
    pub card_slot: gtk::Box,
    pub card_title: gtk::Label,
    pub card_pip: gtk::Box,
    pub promote_btn: gtk::Button,
    pub card_close_btn: gtk::Button,
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
    pub poll_source: Option<glib::SourceId>,
    pub alert_until: Option<Instant>,
    /// Active peek popover, if the VTE is currently reparented into one.
    pub peek_popover: Option<gtk::Popover>,
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
        vte.add_css_class("orbit-mini-vte");

        // Default font scale handled by theme; keep a minimum readable size.
        let font_desc = gtk::pango::FontDescription::from_string("Monospace 11");
        vte.set_font(Some(&font_desc));
        vte.set_font_scale(crate::app::current_font_scale());

        // Sync VTE colors with the Adwaita color scheme.
        apply_vte_theme(&vte);

        // --- Arena tile shell ---
        let tile_frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        tile_frame.add_css_class("orbit-tile");
        tile_frame.set_hexpand(true);
        tile_frame.set_vexpand(true);

        let tile_header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        tile_header.add_css_class("orbit-tile-header");

        let tile_pip = make_pip();
        let tile_title = gtk::Label::new(Some(name));
        tile_title.add_css_class("orbit-tile-title");
        tile_title.set_halign(gtk::Align::Start);
        tile_title.set_hexpand(true);
        tile_title.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let tile_clone_btn = gtk::Button::from_icon_name("edit-copy-symbolic");
        tile_clone_btn.set_tooltip_text(Some("Clone Session (spawn new at this cwd)"));
        tile_clone_btn.add_css_class("flat");
        tile_clone_btn.set_valign(gtk::Align::Center);

        let demote_btn = gtk::Button::from_icon_name("go-next-symbolic");
        demote_btn.set_tooltip_text(Some("Push to Sidebar"));
        demote_btn.add_css_class("flat");
        demote_btn.set_valign(gtk::Align::Center);

        let tile_close_btn = gtk::Button::from_icon_name("window-close-symbolic");
        tile_close_btn.set_tooltip_text(Some("Close Session"));
        tile_close_btn.add_css_class("flat");
        tile_close_btn.set_valign(gtk::Align::Center);

        let tile_emoji = gtk::Label::new(Some(emoji));
        tile_emoji.add_css_class("orbit-session-emoji");
        tile_emoji.set_valign(gtk::Align::Center);

        tile_header.append(&tile_pip);
        tile_header.append(&tile_emoji);
        tile_header.append(&tile_title);
        tile_header.append(&tile_clone_btn);
        tile_header.append(&demote_btn);
        tile_header.append(&tile_close_btn);

        // Clicking the header focuses this tile's VTE.
        {
            let cb = cb.clone();
            let gesture = gtk::GestureClick::new();
            gesture.set_button(gtk::gdk::BUTTON_PRIMARY);
            gesture.connect_pressed(move |_, _, _, _| {
                (cb.borrow())(id, SessionEvent::Focused);
            });
            tile_header.add_controller(gesture);
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
            tile_header.add_controller(drag_source);
        }

        let tile_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
        tile_slot.set_hexpand(true);
        tile_slot.set_vexpand(true);

        tile_frame.append(&tile_header);
        tile_frame.append(&tile_slot);

        // --- Sidebar preview card ---
        let card_frame = gtk::Box::new(gtk::Orientation::Vertical, 4);
        card_frame.add_css_class("orbit-preview-card");
        card_frame.set_hexpand(true);
        card_frame.set_vexpand(false);

        let card_header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        card_header.add_css_class("orbit-card-header");

        let card_pip = make_pip();
        let card_title = gtk::Label::new(Some(name));
        card_title.add_css_class("orbit-preview-title");
        card_title.set_halign(gtk::Align::Start);
        card_title.set_hexpand(true);
        card_title.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let promote_btn = gtk::Button::from_icon_name("go-previous-symbolic");
        promote_btn.set_tooltip_text(Some("Promote to Arena"));
        promote_btn.add_css_class("flat");
        promote_btn.set_valign(gtk::Align::Center);

        let card_close_btn = gtk::Button::from_icon_name("window-close-symbolic");
        card_close_btn.set_tooltip_text(Some("Close Session"));
        card_close_btn.add_css_class("flat");
        card_close_btn.set_valign(gtk::Align::Center);

        let card_emoji = gtk::Label::new(Some(emoji));
        card_emoji.add_css_class("orbit-session-emoji");
        card_emoji.set_valign(gtk::Align::Center);

        card_header.append(&card_pip);
        card_header.append(&card_emoji);
        card_header.append(&card_title);
        card_header.append(&promote_btn);
        card_header.append(&card_close_btn);

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
            card_header.add_controller(drag_source);
        }

        let card_slot = gtk::Box::new(gtk::Orientation::Vertical, 2);
        card_slot.set_hexpand(true);
        card_slot.set_vexpand(false);
        card_slot.set_size_request(-1, 80);
        card_slot.set_overflow(gtk::Overflow::Hidden);
        card_slot.add_css_class("orbit-card-body");

        let metrics_label = gtk::Label::new(None);
        metrics_label.set_halign(gtk::Align::Start);
        metrics_label.set_xalign(0.0);
        metrics_label.add_css_class("orbit-card-metrics");

        // Live VTE is reparented into card_slot before metrics_label when the
        // session is in the sidebar; see place_in_sidebar/place_in_arena/peek.
        card_slot.append(&metrics_label);

        card_frame.append(&card_header);
        card_frame.append(&card_slot);

        let inner = Rc::new(RefCell::new(SessionInner {
            id,
            name: name.to_string(),
            emoji: emoji.to_string(),
            vte: vte.clone(),
            tile_frame,
            tile_header,
            tile_slot,
            tile_title,
            tile_pip,
            demote_btn: demote_btn.clone(),
            tile_clone_btn: tile_clone_btn.clone(),
            tile_close_btn: tile_close_btn.clone(),
            card_frame,
            card_header,
            card_slot,
            card_title,
            card_pip,
            promote_btn: promote_btn.clone(),
            card_close_btn: card_close_btn.clone(),
            metrics_label,
            location: Location::Sidebar, // will be placed by workspace
            pip_state: PipState::Idle,
            elevated: false,
            focused: false,
            is_busy: false,
            state_since: Instant::now(),
            attention: false,
            shell_pid: None,
            poll_source: None,
            alert_until: None,
            peek_popover: None,
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
            promote_btn.connect_clicked(move |_| {
                (cb.borrow())(id, SessionEvent::RequestPromote);
            });
        }
        {
            let cb = cb.clone();
            demote_btn.connect_clicked(move |_| {
                (cb.borrow())(id, SessionEvent::RequestDemote);
            });
        }
        {
            let cb = cb.clone();
            tile_clone_btn.connect_clicked(move |_| {
                (cb.borrow())(id, SessionEvent::RequestClone);
            });
        }
        {
            let cb = cb.clone();
            tile_close_btn.connect_clicked(move |_| {
                (cb.borrow())(id, SessionEvent::RequestClose);
            });
        }
        {
            let cb = cb.clone();
            card_close_btn.connect_clicked(move |_| {
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
                            inner_rc.borrow_mut().shell_pid = Some(pid.0 as i32);
                            Session { inner: inner_rc }.start_polling();
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

    pub fn name(&self) -> String {
        self.inner.borrow().name.clone()
    }

    pub fn emoji(&self) -> String {
        self.inner.borrow().emoji.clone()
    }

    pub fn location(&self) -> Location {
        self.inner.borrow().location
    }

    pub fn vte(&self) -> vte4::Terminal {
        self.inner.borrow().vte.clone()
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

        let (card_frame, vte, vte_widget) = {
            let inner = self.inner.borrow();
            (
                inner.card_frame.clone(),
                inner.vte.clone(),
                inner.vte.clone().upcast::<gtk::Widget>(),
            )
        };

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.set_size_request(640, 400);
        content.add_css_class("orbit-peek-body");

        unparent_from_box(&vte_widget);
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
        let dark = adw::StyleManager::default().is_dark();
        let (add, remove) = if dark {
            ("elevated-dark", "elevated-light")
        } else {
            ("elevated-light", "elevated-dark")
        };
        for header in [&inner.tile_header, &inner.card_header] {
            header.remove_css_class(remove);
            if elevated {
                header.add_css_class(add);
            } else {
                header.remove_css_class(add);
            }
        }
    }

    /// Swap elevated-dark ↔ elevated-light when the theme changes.
    fn refresh_elevated_theme(&self) {
        let inner = self.inner.borrow();
        if !inner.elevated {
            return;
        }
        let dark = adw::StyleManager::default().is_dark();
        let (add, remove) = if dark {
            ("elevated-dark", "elevated-light")
        } else {
            ("elevated-light", "elevated-dark")
        };
        for header in [&inner.tile_header, &inner.card_header] {
            header.remove_css_class(remove);
            header.add_css_class(add);
        }
    }

    pub fn raise_alert(&self) {
        self.set_pip(PipState::Alert);
        self.inner.borrow_mut().alert_until =
            Some(Instant::now() + Duration::from_millis(1500));
    }

    fn start_polling(&self) {
        let weak = Rc::downgrade(&self.inner);
        let source = glib::timeout_add_local(Duration::from_millis(250), move || {
            let Some(inner_rc) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let (pid, alert_until, prev_busy, location) = {
                let inner = inner_rc.borrow();
                (inner.shell_pid, inner.alert_until, inner.is_busy, inner.location)
            };

            // Under an active alert we freeze the pip state but still let
            // the preview tick so long-running demoted sessions don't appear
            // stale on-screen.
            let alert_active = alert_until
                .map(|u| Instant::now() < u)
                .unwrap_or(false);
            if !alert_active {
                if alert_until.is_some() {
                    inner_rc.borrow_mut().alert_until = None;
                }
            }

            let session = Session { inner: inner_rc };
            if let Some(pid) = pid {
                let now_busy = !is_terminal_idle(pid);
                if !alert_active {
                    session.set_pip(if now_busy { PipState::Busy } else { PipState::Idle });
                }
                if now_busy != prev_busy {
                    {
                        let mut inner = session.inner.borrow_mut();
                        inner.is_busy = now_busy;
                        inner.state_since = Instant::now();
                    }
                    // Busy→Idle in the dock = "needs attention": the process
                    // just finished or is waiting for input while off-stage.
                    if !now_busy && location == Location::Sidebar {
                        session.set_attention(true);
                    }
                }
                session.set_elevated(is_foreground_elevated(pid));
            }

            if location == Location::Sidebar {
                session.refresh_preview();
            }
            glib::ControlFlow::Continue
        });
        self.inner.borrow_mut().poll_source = Some(source);
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

    pub fn set_name(&self, name: &str) {
        let mut inner = self.inner.borrow_mut();
        inner.name = name.into();
        inner.tile_title.set_text(name);
        inner.card_title.set_text(name);
    }
}

/// Parse pgrp and tpgid from /proc/[pid]/stat.
fn parse_pgrp_tpgid(pid: i32) -> Option<(i32, i32)> {
    let data = std::fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
    let pos = data.rfind(')')?;
    let rest = &data[pos + 2..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    if fields.len() <= 5 {
        return None;
    }
    let pgrp: i32 = fields[2].parse().ok()?;
    let tpgid: i32 = fields[5].parse().ok()?;
    Some((pgrp, tpgid))
}

/// Check whether the terminal is waiting for user input.
/// Handles nested shells (e.g. `su root` spawning a new bash).
fn is_terminal_idle(shell_pid: i32) -> bool {
    let Some((pgrp, tpgid)) = parse_pgrp_tpgid(shell_pid) else {
        return true;
    };
    if pgrp == tpgid {
        return true; // Original shell is the foreground → idle.
    }
    // Something else is foreground. If the fg leader is a shell at a
    // prompt (e.g. root bash from `su`), the terminal is still "idle".
    if tpgid > 0 {
        if let Ok(comm) = std::fs::read_to_string(format!("/proc/{}/comm", tpgid)) {
            let name = comm.trim();
            // Strip leading '-' (login shell convention, e.g. "-bash").
            let name = name.strip_prefix('-').unwrap_or(name);
            const SHELLS: &[&str] = &[
                "bash", "zsh", "fish", "sh", "dash", "ksh", "csh", "tcsh",
                "nu", "nushell", "elvish", "ion", "xonsh", "pwsh",
            ];
            if SHELLS.contains(&name) {
                // The fg leader is a shell. Check that IT is the foreground
                // (it hasn't spawned a child command that took over).
                if let Some((fg_pgrp, fg_tpgid)) = parse_pgrp_tpgid(tpgid) {
                    return fg_pgrp == fg_tpgid;
                }
            }
        }
    }
    false
}

/// Full command line of the foreground process currently running in the
/// terminal, or None when the shell itself is at a prompt. Descends through
/// nested shells (e.g. `su -` spawning bash) to reach the actual command.
///
/// Reads `/proc/<pid>/cmdline` rather than `/proc/<pid>/comm` so the user
/// sees the real argv — `comm` is truncated to 15 chars and reflects the
/// thread name for runtimes that set one (Node.js → "MainThread").
fn foreground_command(shell_pid: i32) -> Option<String> {
    const SHELLS: &[&str] = &[
        "bash", "zsh", "fish", "sh", "dash", "ksh", "csh", "tcsh",
        "nu", "nushell", "elvish", "ion", "xonsh", "pwsh",
    ];
    let (pgrp, tpgid) = parse_pgrp_tpgid(shell_pid)?;
    if tpgid <= 0 || pgrp == tpgid {
        return None;
    }
    // Use comm only for the shell-detection check (recurse through nested
    // shells). Comm is cheap and well-suited for that purpose.
    let comm = std::fs::read_to_string(format!("/proc/{}/comm", tpgid)).ok()?;
    let raw = comm.trim();
    let name = raw.strip_prefix('-').unwrap_or(raw);
    if SHELLS.contains(&name) {
        return foreground_command(tpgid);
    }
    // Real command: read argv from /proc/<pid>/cmdline (null-separated).
    let cmdline_bytes = std::fs::read(format!("/proc/{}/cmdline", tpgid)).ok()?;
    let parts: Vec<String> = cmdline_bytes
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect();
    if parts.is_empty() {
        return Some(name.to_string());
    }
    Some(parts.join(" "))
}

/// Check whether the terminal's foreground process is running with root
/// privileges (euid == 0). Used to tint the header bar red.
fn is_foreground_elevated(shell_pid: i32) -> bool {
    let Some((_pgrp, tpgid)) = parse_pgrp_tpgid(shell_pid) else {
        return false;
    };
    if tpgid <= 0 {
        return false;
    }
    let Ok(status) = std::fs::read_to_string(format!("/proc/{}/status", tpgid)) else {
        return false;
    };
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            let fields: Vec<&str> = rest.split_whitespace().collect();
            // Uid: real effective saved filesystem
            if let Some(euid_str) = fields.get(1) {
                return euid_str.parse::<u32>().unwrap_or(u32::MAX) == 0;
            }
        }
    }
    false
}

/// Extract the source session id from a DropTarget's current drag.
pub(crate) fn extract_source_id(dt: &gtk::DropTarget) -> Option<u32> {
    let drop = dt.current_drop()?;
    let drag = drop.drag()?;
    let content = drag.content();
    content.value(<u32 as glib::types::StaticType>::static_type()).ok()?.get::<u32>().ok()
}

/// Detach `widget` from its current parent if that parent is a `gtk::Box`.
/// Used to reparent the VTE between tile_slot / card_slot / peek popover.
fn unparent_from_box(widget: &gtk::Widget) {
    if let Some(parent) = widget.parent() {
        if let Some(parent_box) = parent.downcast_ref::<gtk::Box>() {
            parent_box.remove(widget);
        }
    }
}

/// Row count of the VTE when rendered as the dock's read-only preview.
/// `set_size_request` only clamps the minimum — to actually *cap* VTE's
/// natural height we have to pin its internal rows via `set_size`, which
/// also sends SIGWINCH to the shell. Fine for line-based output; TUIs will
/// redraw on demote/peek/promote.
const PREVIEW_VTE_ROWS: i64 = 4;

/// Toggle VTE between interactive (arena tile / peek popover) and read-only
/// preview (sidebar card). Read-only mode blocks keystrokes, focus theft, and
/// the blinking cursor so the card reads as a live snapshot, and pins VTE to
/// a small row count so the card doesn't stretch to fill the dock.
fn set_vte_interactive(vte: &vte4::Terminal, interactive: bool) {
    vte.set_input_enabled(interactive);
    vte.set_focusable(interactive);
    vte.set_can_focus(interactive);
    vte.set_can_target(interactive);
    vte.set_cursor_blink_mode(if interactive {
        vte4::CursorBlinkMode::System
    } else {
        vte4::CursorBlinkMode::Off
    });
    if interactive {
        vte.set_vexpand(true);
    } else {
        vte.set_vexpand(false);
        let cols = vte.column_count().max(20);
        vte.set_size(cols, PREVIEW_VTE_ROWS);
    }
}

/// Compact relative-duration format: `45s`, `3m`, `1h 12m`.
fn format_duration_compact(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{}s", s)
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else {
        format!("{}h {}m", s / 3600, (s / 60) % 60)
    }
}

fn make_pip() -> gtk::Box {
    let pip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    pip.add_css_class("orbit-pip");
    pip.add_css_class("idle");
    pip.set_valign(gtk::Align::Center);
    pip.set_halign(gtk::Align::Center);
    pip
}

/// Set VTE terminal foreground/background to match the current Adwaita theme.
fn apply_vte_theme(vte: &vte4::Terminal) {
    let dark = adw::StyleManager::default().is_dark();
    let (fg, bg) = if dark {
        // Adwaita dark: light text on dark bg
        (gtk::gdk::RGBA::new(0.93, 0.93, 0.93, 1.0),
         gtk::gdk::RGBA::new(0.12, 0.12, 0.12, 1.0))
    } else {
        // Adwaita light: dark text on light bg
        (gtk::gdk::RGBA::new(0.2, 0.2, 0.2, 1.0),
         gtk::gdk::RGBA::new(0.98, 0.98, 0.98, 1.0))
    };
    vte.set_color_foreground(&fg);
    vte.set_color_background(&bg);
}
