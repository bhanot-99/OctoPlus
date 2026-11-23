//! The pages. Each one builds its widgets once and redraws from [`AppState`] when told.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;

use neuroos_octopit_core::controller::Controller;
use neuroos_octopit_core::state::{AppState, Changed};

pub mod activity;
pub mod backends;
pub mod chat;
pub mod overview;
pub mod privacy;
pub mod settings;

/// The pages, in sidebar order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageId {
    /// Is everything healthy?
    Overview,
    /// Talk to NeuroOS.
    Chat,
    /// Models and GPU.
    Backends,
    /// Speed, load and the audit trail.
    Activity,
    /// The proof that nothing leaves the machine.
    Privacy,
    /// Theme and motion.
    Settings,
}

impl PageId {
    /// The pages in the sidebar (settings lives in the header).
    pub const SIDEBAR: [PageId; 5] = [
        PageId::Overview,
        PageId::Chat,
        PageId::Backends,
        PageId::Activity,
        PageId::Privacy,
    ];

    /// Name used by the stack.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            PageId::Overview => "overview",
            PageId::Chat => "chat",
            PageId::Backends => "backends",
            PageId::Activity => "activity",
            PageId::Privacy => "privacy",
            PageId::Settings => "settings",
        }
    }

    /// Name shown to people.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            PageId::Overview => "Overview",
            PageId::Chat => "Chat",
            PageId::Backends => "Backends",
            PageId::Activity => "Activity",
            PageId::Privacy => "Privacy",
            PageId::Settings => "Settings",
        }
    }

    /// Icon name from the bundled set.
    #[must_use]
    pub const fn icon(self) -> &'static str {
        match self {
            PageId::Overview => "octopit-overview-symbolic",
            PageId::Chat => "octopit-chat-symbolic",
            PageId::Backends => "octopit-backends-symbolic",
            PageId::Activity => "octopit-activity-symbolic",
            PageId::Privacy => "octopit-privacy-symbolic",
            PageId::Settings => "octopit-settings-symbolic",
        }
    }
}

/// What every page can use.
#[derive(Clone)]
pub struct Ctx {
    /// Sends commands to the background half.
    pub controller: Controller,
    /// Switches page.
    pub navigate: Rc<dyn Fn(PageId)>,
    /// Shows a short notice.
    pub toast: Rc<dyn Fn(&str)>,
    /// The shared state (read it in `refresh`; the window owns writes).
    pub state: Rc<RefCell<AppState>>,
}

/// One page of the window.
pub trait Page {
    /// The page's root widget.
    fn widget(&self) -> gtk::Widget;
    /// Redraws what `changed` touches from `state`.
    fn refresh(&self, state: &AppState, changed: Changed);
    /// Called when the page becomes visible (focus the right control).
    fn shown(&self) {}
}
