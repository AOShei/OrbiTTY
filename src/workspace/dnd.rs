use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use super::{Workspace, WorkspaceInner};

impl Workspace {
    pub(super) fn install_background_drop_targets(&self) {
        let (arena_grid, empty_state, sidebar_scroller) = {
            let inner = self.inner.borrow();
            (
                inner.arena.widget(),
                inner.arena.empty_state_widget(),
                inner.sidebar.scroller_widget(),
            )
        };

        // Arena grid: hover shows phantom when sidebar card is dragged over.
        {
            let weak: Weak<RefCell<WorkspaceInner>> = Rc::downgrade(&self.inner);
            let dt = gtk::DropTarget::new(
                <u32 as glib::types::StaticType>::static_type(),
                gtk::gdk::DragAction::MOVE,
            );
            {
                let weak = weak.clone();
                dt.connect_enter(move |dt, x, y| {
                    if let Some(inner_rc) = weak.upgrade() {
                        let guard = inner_rc.borrow().suppressing_hover.clone();
                        if guard.get() {
                            return gtk::gdk::DragAction::MOVE;
                        }
                        let arena = inner_rc.borrow().arena.clone();
                        // Only show phantom for sidebar→arena drags, not arena→arena.
                        let source_in_arena = crate::session::extract_source_id(dt)
                            .map(|sid| arena.contains(sid))
                            .unwrap_or(false);
                        if source_in_arena {
                            return gtk::gdk::DragAction::MOVE;
                        }
                        let gen_rc = inner_rc.borrow().drag_hover_gen.clone();
                        gen_rc.set(gen_rc.get().wrapping_add(1));
                        let gen = gen_rc.get();
                        if !arena.is_full() {
                            let slot = arena.slot_from_coords(x, y);
                            glib::idle_add_local_once(move || {
                                if gen_rc.get() != gen {
                                    return;
                                }
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
                        if guard.get() {
                            return gtk::gdk::DragAction::MOVE;
                        }
                        let arena = inner_rc.borrow().arena.clone();
                        let source_in_arena = crate::session::extract_source_id(dt)
                            .map(|sid| arena.contains(sid))
                            .unwrap_or(false);
                        if source_in_arena {
                            return gtk::gdk::DragAction::MOVE;
                        }
                        if !arena.is_full() {
                            let slot = arena.slot_from_coords(x, y);
                            if slot != arena.phantom_slot() || !arena.has_phantom() {
                                let gen_rc = inner_rc.borrow().drag_hover_gen.clone();
                                let gen = gen_rc.get();
                                glib::idle_add_local_once(move || {
                                    if gen_rc.get() != gen {
                                        return;
                                    }
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
                        if guard.get() {
                            return;
                        }
                        let gen_rc = inner_rc.borrow().drag_hover_gen.clone();
                        let gen = gen_rc.get();
                        let ws = Workspace { inner: inner_rc };
                        // Use a short timeout so that a subsequent enter (which
                        // bumps gen) can cancel this clear before it executes.
                        glib::timeout_add_local_once(
                            std::time::Duration::from_millis(50),
                            move || {
                                if gen_rc.get() != gen {
                                    return;
                                }
                                guard.set(true);
                                ws.clear_all_previews();
                                glib::idle_add_local_once(move || {
                                    guard.set(false);
                                });
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
            let weak: Weak<RefCell<WorkspaceInner>> = Rc::downgrade(&self.inner);
            let dt = gtk::DropTarget::new(
                <u32 as glib::types::StaticType>::static_type(),
                gtk::gdk::DragAction::MOVE,
            );
            dt.connect_drop(move |_t, value, _x, _y| {
                let Ok(source_id) = value.get::<u32>() else {
                    return false;
                };
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
            let weak: Weak<RefCell<WorkspaceInner>> = Rc::downgrade(&self.inner);
            let dt = gtk::DropTarget::new(
                <u32 as glib::types::StaticType>::static_type(),
                gtk::gdk::DragAction::MOVE,
            );
            {
                let weak = weak.clone();
                dt.connect_enter(move |dt, _x, y| {
                    if let Some(inner_rc) = weak.upgrade() {
                        let guard = inner_rc.borrow().suppressing_hover.clone();
                        if guard.get() {
                            return gtk::gdk::DragAction::MOVE;
                        }
                        let sidebar = inner_rc.borrow().sidebar.clone();
                        let arena = inner_rc.borrow().arena.clone();
                        let scroll_y = sidebar.scroller_widget().vadjustment().value();
                        let list_y = y + scroll_y;
                        let gen_rc = inner_rc.borrow().drag_hover_gen.clone();
                        gen_rc.set(gen_rc.get().wrapping_add(1));
                        let gen = gen_rc.get();
                        // If source is an arena session, preview the shrink.
                        let source_id = crate::session::extract_source_id(dt);
                        let source_in_arena = source_id.map(|sid| arena.contains(sid)).unwrap_or(false);
                        glib::idle_add_local_once(move || {
                            if gen_rc.get() != gen {
                                return;
                            }
                            guard.set(true);
                            if source_in_arena {
                                if let Some(sid) = source_id {
                                    arena.preview_remove(sid);
                                }
                            }
                            sidebar.show_placeholder(list_y);
                            glib::idle_add_local_once(move || {
                                guard.set(false);
                            });
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
                        if guard.get() {
                            return gtk::gdk::DragAction::MOVE;
                        }
                        let sidebar = inner_rc.borrow().sidebar.clone();
                        let scroll_y = sidebar.scroller_widget().vadjustment().value();
                        let list_y = y + scroll_y;
                        sidebar.move_placeholder(list_y);
                    }
                    gtk::gdk::DragAction::MOVE
                });
            }
            {
                let weak = weak.clone();
                dt.connect_leave(move |_dt| {
                    if let Some(inner_rc) = weak.upgrade() {
                        let guard = inner_rc.borrow().suppressing_hover.clone();
                        if guard.get() {
                            return;
                        }
                        let gen_rc = inner_rc.borrow().drag_hover_gen.clone();
                        let gen = gen_rc.get();
                        let ws = Workspace { inner: inner_rc };
                        glib::idle_add_local_once(move || {
                            if gen_rc.get() != gen {
                                return;
                            }
                            guard.set(true);
                            ws.clear_all_previews();
                            glib::idle_add_local_once(move || {
                                guard.set(false);
                            });
                        });
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
                        // Capture insertion index from placeholder before clearing.
                        let insert_idx = sidebar.placeholder_insert_index();
                        ws.clear_all_previews();
                        if arena.contains(source_id) {
                            ws.demote(source_id);
                            // demote() appends to the end — move to the
                            // position the placeholder was showing.
                            sidebar.reorder_to_index(source_id, insert_idx);
                        } else if sidebar.contains(source_id) {
                            sidebar.reorder_to_index(source_id, insert_idx);
                        }
                    }
                    true
                });
            }
            sidebar_scroller.add_controller(dt);
        }
    }

    /// Dispatch a drag-and-drop landing on a session target (`target_id`) from
    /// a dragged session (`source_id`). Branches on the region each id lives in
    /// and whether the arena has room; see the verification table in the plan.
    pub(super) fn handle_drop(&self, source_id: u32, target_id: u32) {
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
            (true, true, _, _) => {
                arena.swap_sessions(source_id, target_id);
            }

            // Sidebar → arena.
            (_, true, true, _) => {
                if !arena.is_full() {
                    // Room available: insert at the target's position.
                    let target_idx = arena.session_ids().iter().position(|&x| x == target_id);
                    let slot = target_idx.unwrap_or(arena.count());
                    self.promote_at(source_id, Some(slot));
                } else {
                    // Arena full: cross-region swap at the target tile's slot.
                    let Some(dragged) = sidebar.remove(source_id) else {
                        return;
                    };
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
                let Some(target) = sidebar.remove(target_id) else {
                    return;
                };
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

    /// Handle drag hover entering a session's drop zone.
    /// Uses tile-local coordinates translated to grid space for accurate
    /// phantom slot calculation.
    pub(super) fn handle_drag_hover_enter(&self, source_id: u32, target_id: u32, tile_x: f64, tile_y: f64) {
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
                if gen_rc.get() != gen {
                    return;
                }
                guard.set(true);
                arena.ensure_phantom_at(slot);
                glib::idle_add_local_once(move || {
                    guard.set(false);
                });
            });
        }
    }

    /// Handle continuous cursor motion within a tile during drag.
    /// Updates phantom position based on actual cursor location in the grid.
    pub(super) fn handle_drag_hover_motion(&self, source_id: u32, target_id: u32, tile_x: f64, tile_y: f64) {
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
                if gen_rc.get() != gen {
                    return;
                }
                guard.set(true);
                arena.ensure_phantom_at(slot);
                glib::idle_add_local_once(move || {
                    guard.set(false);
                });
            });
        }
    }

    /// Handle drag hover leaving a session's drop zone.
    /// Phantom cleanup is handled by the grid leave timeout and DragEnded;
    /// per-tile leave only removes CSS classes (done in session.rs).
    pub(super) fn handle_drag_hover_leave(&self, _source_id: u32, _target_id: u32) {
        // No-op: don't clear phantom here to avoid fighting with enter/motion
        // handlers after rebuild shifts tile positions.
    }

    /// Reset all drag-related visual previews to their normal state.
    pub(super) fn clear_all_previews(&self) {
        let inner = self.inner.borrow();
        inner.arena.clear_all_previews();
        inner.sidebar.hide_placeholder();
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
}
