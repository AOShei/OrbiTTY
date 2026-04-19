use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::RefCell;
use std::rc::Rc;

use crate::session::Session;

/// The right-hand "Monitoring Sidebar" holding inactive session cards.
#[derive(Clone)]
pub struct Sidebar {
    pub root: gtk::Box,
    pub(crate) list: gtk::Box,
    pub(crate) scroller: gtk::ScrolledWindow,
    pub sessions: Rc<RefCell<Vec<Session>>>,
}

impl Sidebar {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("orbit-sidebar");
        root.set_width_request(260);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header.set_margin_bottom(6);

        let title = gtk::Label::new(Some("Monitoring"));
        title.add_css_class("heading");
        title.set_halign(gtk::Align::Start);
        title.set_hexpand(true);

        header.append(&title);

        let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
        list.set_hexpand(true);

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .build();
        scroller.set_child(Some(&list));

        root.append(&header);
        root.append(&scroller);

        Sidebar {
            root,
            list,
            scroller,
            sessions: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub fn widget(&self) -> gtk::Box {
        self.root.clone()
    }

    pub fn add(&self, session: Session) {
        self.list.append(&session.card_frame());
        self.sessions.borrow_mut().push(session);
    }

    pub fn remove(&self, id: u32) -> Option<Session> {
        let mut v = self.sessions.borrow_mut();
        let pos = v.iter().position(|s| s.id() == id)?;
        let session = v.remove(pos);
        drop(v);
        self.list.remove(&session.card_frame());
        Some(session)
    }

    pub fn contains(&self, id: u32) -> bool {
        self.sessions.borrow().iter().any(|s| s.id() == id)
    }

    pub fn session_ids(&self) -> Vec<u32> {
        self.sessions.borrow().iter().map(|s| s.id()).collect()
    }

    #[allow(dead_code)]
    pub fn scroller(&self) -> gtk::ScrolledWindow {
        self.scroller.clone()
    }
}
