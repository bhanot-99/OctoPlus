//! The Backends page. PLACEHOLDER until its build task lands (docs/plans/phase-11-octopit-plan.md).

use gtk::prelude::*;
use gtk4 as gtk;

use neuroos_octopit_core::state::{AppState, Changed};

use super::{Ctx, Page};

/// The Backends page.
pub struct BackendsPage {
    root: gtk::Box,
}

impl BackendsPage {
    /// Builds the page.
    #[must_use]
    pub fn new(_ctx: &Ctx) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&crate::widgets::label("Backends", &["title-page", "page"]));
        Self { root }
    }
}

impl Page for BackendsPage {
    fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }
    fn refresh(&self, _state: &AppState, _changed: Changed) {}
}
