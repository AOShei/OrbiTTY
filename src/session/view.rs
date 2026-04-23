use gtk4 as gtk;
use libadwaita as adw;
use vte4::prelude::*;

pub(super) struct SessionWidgets {
    pub tile_frame: gtk::Box,
    pub tile_header: gtk::Box,
    pub tile_slot: gtk::Box,
    pub tile_title: gtk::Label,
    pub tile_pip: gtk::Box,
    pub tile_clone_btn: gtk::Button,
    pub demote_btn: gtk::Button,
    pub tile_close_btn: gtk::Button,
    pub card_frame: gtk::Box,
    pub card_header: gtk::Box,
    pub card_slot: gtk::Box,
    pub card_title: gtk::Label,
    pub card_pip: gtk::Box,
    pub promote_btn: gtk::Button,
    pub card_close_btn: gtk::Button,
    pub metrics_label: gtk::Label,
}

pub(super) fn configure_terminal(vte: &vte4::Terminal) {
    vte.add_css_class("orbit-mini-vte");

    // Default font scale handled by theme; keep a minimum readable size.
    let font_desc = gtk::pango::FontDescription::from_string("Monospace 11");
    vte.set_font(Some(&font_desc));
    vte.set_font_scale(crate::app::current_font_scale());

    // Sync VTE colors with the Adwaita color scheme.
    apply_vte_theme(vte);
}

pub(super) fn build_session_widgets(name: &str, emoji: &str) -> SessionWidgets {
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

    let tile_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
    tile_slot.set_hexpand(true);
    tile_slot.set_vexpand(true);

    tile_frame.append(&tile_header);
    tile_frame.append(&tile_slot);

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

    SessionWidgets {
        tile_frame,
        tile_header,
        tile_slot,
        tile_title,
        tile_pip,
        tile_clone_btn,
        demote_btn,
        tile_close_btn,
        card_frame,
        card_header,
        card_slot,
        card_title,
        card_pip,
        promote_btn,
        card_close_btn,
        metrics_label,
    }
}

pub(super) fn sync_elevated_headers(
    tile_header: &gtk::Box,
    card_header: &gtk::Box,
    elevated: bool,
) {
    let (add, remove) = elevated_css_classes();
    for header in [tile_header, card_header] {
        header.remove_css_class(remove);
        if elevated {
            header.add_css_class(add);
        } else {
            header.remove_css_class(add);
        }
    }
}

pub(super) fn unparent_from_box(widget: &gtk::Widget) {
    if let Some(parent) = widget.parent() {
        if let Some(parent_box) = parent.downcast_ref::<gtk::Box>() {
            parent_box.remove(widget);
        }
    }
}

/// Toggle VTE between interactive (arena tile / peek popover) and read-only
/// preview (sidebar card). Read-only mode blocks keystrokes, focus theft, and
/// the blinking cursor so the card reads as a live snapshot, and pins VTE to
/// a small row count so the card doesn't stretch to fill the dock.
pub(super) fn set_vte_interactive(vte: &vte4::Terminal, interactive: bool) {
    const PREVIEW_VTE_ROWS: i64 = 4;

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

/// Set VTE terminal foreground/background to match the current Adwaita theme.
pub(super) fn apply_vte_theme(vte: &vte4::Terminal) {
    let dark = adw::StyleManager::default().is_dark();
    let (fg, bg) = if dark {
        // Adwaita dark: light text on dark bg
        (
            gtk::gdk::RGBA::new(0.93, 0.93, 0.93, 1.0),
            gtk::gdk::RGBA::new(0.12, 0.12, 0.12, 1.0),
        )
    } else {
        // Adwaita light: dark text on light bg
        (
            gtk::gdk::RGBA::new(0.2, 0.2, 0.2, 1.0),
            gtk::gdk::RGBA::new(0.98, 0.98, 0.98, 1.0),
        )
    };
    vte.set_color_foreground(&fg);
    vte.set_color_background(&bg);
}

fn make_pip() -> gtk::Box {
    let pip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    pip.add_css_class("orbit-pip");
    pip.add_css_class("idle");
    pip.set_valign(gtk::Align::Center);
    pip.set_halign(gtk::Align::Center);
    pip
}

fn elevated_css_classes() -> (&'static str, &'static str) {
    if adw::StyleManager::default().is_dark() {
        ("elevated-dark", "elevated-light")
    } else {
        ("elevated-light", "elevated-dark")
    }
}
