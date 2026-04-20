use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4 as gtk;
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
    /// A drag carrying `source_id` entered this session's drop zone.
    DragHoverEnter(u32),
    /// A drag left this session's drop zone.
    DragHoverLeave(u32),
}

pub struct SessionInner {
    pub id: u32,
    pub name: String,
    pub vte: vte4::Terminal,

    // Arena tile (outer frame + header + content slot).
    pub tile_frame: gtk::Box,
    pub tile_slot: gtk::Box,
    pub tile_title: gtk::Label,
    pub tile_pip: gtk::Box,
    pub demote_btn: gtk::Button,
    pub tile_clone_btn: gtk::Button,
    pub tile_close_btn: gtk::Button,

    // Sidebar card.
    pub card_frame: gtk::Box,
    pub card_slot: gtk::Box,
    pub card_title: gtk::Label,
    pub card_pip: gtk::Box,
    pub promote_btn: gtk::Button,
    pub card_close_btn: gtk::Button,

    pub location: Location,
    pub pip_state: PipState,
    pub focused: bool,
    pub shell_pid: Option<i32>,
    pub poll_source: Option<glib::SourceId>,
    pub alert_until: Option<Instant>,
}

#[derive(Clone)]
pub struct Session {
    pub inner: Rc<RefCell<SessionInner>>,
}

impl Session {
    pub fn new(id: u32, name: &str, cwd: Option<&str>, cb: SessionCallback) -> Self {
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

        tile_header.append(&tile_pip);
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

        card_header.append(&card_pip);
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

        let card_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card_slot.set_hexpand(true);
        card_slot.set_vexpand(false);
        card_slot.set_size_request(-1, 140);
        card_slot.set_overflow(gtk::Overflow::Hidden);

        card_frame.append(&card_header);
        card_frame.append(&card_slot);

        let inner = Rc::new(RefCell::new(SessionInner {
            id,
            name: name.to_string(),
            vte: vte.clone(),
            tile_frame,
            tile_slot,
            tile_title,
            tile_pip,
            demote_btn: demote_btn.clone(),
            tile_clone_btn: tile_clone_btn.clone(),
            tile_close_btn: tile_close_btn.clone(),
            card_frame,
            card_slot,
            card_title,
            card_pip,
            promote_btn: promote_btn.clone(),
            card_close_btn: card_close_btn.clone(),
            location: Location::Sidebar, // will be placed by workspace
            pip_state: PipState::Idle,
            focused: false,
            shell_pid: None,
            poll_source: None,
            alert_until: None,
        }));

        let session = Session { inner };

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
            // Bell → alert pip.
            let cb = cb.clone();
            vte.connect_bell(move |_| {
                (cb.borrow())(id, SessionEvent::Bell);
            });
        }
        {
            // Shell exit (e.g. user typed `exit`) → close the session.
            let cb = cb.clone();
            vte.connect_child_exited(move |_, _status| {
                (cb.borrow())(id, SessionEvent::RequestClose);
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
                drop_target.connect_enter(move |dt, _x, _y| {
                    tile_frame.add_css_class("drop-hover");
                    if let Some(source_id) = extract_source_id(dt) {
                        if source_id != id {
                            (cb.borrow())(id, SessionEvent::DragHoverEnter(source_id));
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
        {
            let cb = cb.clone();
            let card_frame = session.inner.borrow().card_frame.clone();
            let drop_target = gtk::DropTarget::new(
                <u32 as glib::types::StaticType>::static_type(),
                gtk::gdk::DragAction::MOVE,
            );
            {
                let cb = cb.clone();
                let card_frame = card_frame.clone();
                drop_target.connect_enter(move |dt, _x, _y| {
                    card_frame.add_css_class("drop-hover");
                    if let Some(source_id) = extract_source_id(dt) {
                        if source_id != id {
                            (cb.borrow())(id, SessionEvent::DragHoverEnter(source_id));
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
        if let Some(parent) = vte_widget.parent() {
            if let Some(parent_box) = parent.downcast_ref::<gtk::Box>() {
                parent_box.remove(&vte_widget);
            }
        }
        tile_slot.append(&vte_widget);
        self.inner.borrow_mut().location = Location::Arena;
    }

    pub fn place_in_sidebar(&self) {
        let (vte_widget, card_slot) = {
            let inner = self.inner.borrow();
            (
                inner.vte.clone().upcast::<gtk::Widget>(),
                inner.card_slot.clone(),
            )
        };
        if let Some(parent) = vte_widget.parent() {
            if let Some(parent_box) = parent.downcast_ref::<gtk::Box>() {
                parent_box.remove(&vte_widget);
            }
        }
        card_slot.append(&vte_widget);
        self.inner.borrow_mut().location = Location::Sidebar;
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

    pub fn raise_alert(&self) {
        self.set_pip(PipState::Alert);
        self.inner.borrow_mut().alert_until =
            Some(Instant::now() + Duration::from_millis(1500));
    }

    fn start_polling(&self) {
        let weak = Rc::downgrade(&self.inner);
        let source = glib::timeout_add_local(Duration::from_millis(500), move || {
            let Some(inner_rc) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let (pid, alert_until) = {
                let inner = inner_rc.borrow();
                (inner.shell_pid, inner.alert_until)
            };
            // Don't override an active alert.
            if let Some(until) = alert_until {
                if Instant::now() < until {
                    return glib::ControlFlow::Continue;
                }
                inner_rc.borrow_mut().alert_until = None;
            }
            let session = Session { inner: inner_rc };
            if let Some(pid) = pid {
                if is_shell_idle(pid) {
                    session.set_pip(PipState::Idle);
                } else {
                    session.set_pip(PipState::Busy);
                }
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

/// Check whether the shell is the foreground process of its terminal,
/// i.e. waiting for user input rather than running a child command.
fn is_shell_idle(pid: i32) -> bool {
    let path = format!("/proc/{}/stat", pid);
    let Ok(data) = std::fs::read_to_string(&path) else {
        return true;
    };
    // /proc/[pid]/stat: pid (comm) state ppid pgrp session tty_nr tpgid ...
    // comm may contain spaces/parens, so find the last ')'.
    let Some(pos) = data.rfind(')') else {
        return true;
    };
    let rest = &data[pos + 2..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // fields[2] = pgrp, fields[5] = tpgid (foreground process group of tty)
    if fields.len() > 5 {
        let pgrp: i32 = fields[2].parse().unwrap_or(0);
        let tpgid: i32 = fields[5].parse().unwrap_or(-1);
        pgrp == tpgid
    } else {
        true
    }
}

/// Extract the source session id from a DropTarget's current drag.
fn extract_source_id(dt: &gtk::DropTarget) -> Option<u32> {
    let drop = dt.current_drop()?;
    let drag = drop.drag()?;
    let content = drag.content();
    content.value(<u32 as glib::types::StaticType>::static_type()).ok()?.get::<u32>().ok()
}

fn make_pip() -> gtk::Box {
    let pip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    pip.add_css_class("orbit-pip");
    pip.add_css_class("idle");
    pip.set_valign(gtk::Align::Center);
    pip.set_halign(gtk::Align::Center);
    pip
}
