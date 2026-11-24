//! The mic (docs/design/octopit-ux.md §3, §10): one logic, two faces. The floating button
//! ([`MicFab`]) is on every page except Overview; the big gradient [`VoiceCard`] lives on
//! Overview. Both send only start, stop and cancel; neither ever touches audio.

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;

use octopit_core::controller::{Command, Controller};
use octopit_core::voice::{MicAction, VoiceState, VoiceView};

use crate::widgets::{bar, icon, label, set_bar};

const STATE_CLASSES: [&str; 8] = [
    "unavailable",
    "off",
    "idle",
    "listening",
    "thinking",
    "speaking",
    "followup",
    "busy",
];

const fn class_for(s: VoiceState) -> &'static str {
    match s {
        VoiceState::Unavailable => "unavailable",
        VoiceState::Off => "off",
        VoiceState::Idle => "idle",
        VoiceState::Listening => "listening",
        VoiceState::Thinking => "thinking",
        VoiceState::Speaking => "speaking",
        VoiceState::FollowUp => "followup",
        VoiceState::Busy => "busy",
    }
}

const fn glyph_for(v: &VoiceView) -> &'static str {
    match v.press() {
        MicAction::Start => "octopit-mic-symbolic",
        MicAction::Stop | MicAction::Cancel => "octopit-stop-symbolic",
        MicAction::None => "octopit-mic-off-symbolic",
    }
}

/// What to say under the button for each state.
const fn hint_for(s: VoiceState) -> &'static str {
    match s {
        VoiceState::Idle | VoiceState::FollowUp => "Or press Ctrl+Space anywhere in OctoPit",
        VoiceState::Listening => "Press again to finish",
        VoiceState::Thinking | VoiceState::Speaking => "Press to stop",
        VoiceState::Busy => "Close the other app, then try again",
        VoiceState::Off => "Start it: systemctl --user start octoplus-voice",
        VoiceState::Unavailable => "Waiting for the voice part",
    }
}

/// The button and its logic, shared by both faces.
struct MicButton {
    button: gtk::Button,
    glyph: gtk::Image,
    action: Rc<Cell<MicAction>>,
    last_state: Cell<Option<VoiceState>>,
    /// Where screen-reader announcements go.
    announcer: gtk::Label,
}

impl MicButton {
    fn new(controller: &Controller, fab: bool) -> Self {
        let button = gtk::Button::new();
        button.add_css_class("mic-button");
        if fab {
            button.add_css_class("fab");
        }
        button.set_halign(gtk::Align::Center);
        button.set_valign(gtk::Align::Center);
        let glyph = icon("octopit-mic-symbolic", if fab { 24 } else { 32 });
        button.set_child(Some(&glyph));
        let action = Rc::new(Cell::new(MicAction::None));
        {
            let action = Rc::clone(&action);
            let ctl = controller.clone();
            button.connect_clicked(move |_| {
                let a = action.get();
                if a != MicAction::None {
                    let _ = ctl.send(Command::Mic(a));
                }
            });
        }
        let announcer = label("", &[]);
        announcer.set_visible(false);
        Self {
            button,
            glyph,
            action,
            last_state: Cell::new(None),
            announcer,
        }
    }

    fn refresh(&self, v: &VoiceView) {
        self.action.set(v.press());
        for c in STATE_CLASSES {
            self.button.remove_css_class(c);
        }
        self.button.add_css_class(class_for(v.state));
        self.button.set_sensitive(v.press() != MicAction::None);
        self.glyph.set_icon_name(Some(glyph_for(v)));
        self.button
            .update_property(&[gtk::accessible::Property::Label(v.button_label())]);
        self.button.set_tooltip_text(Some(v.button_label()));
        if self.last_state.get() != Some(v.state) {
            self.last_state.set(Some(v.state));
            self.announcer
                .announce(v.label(), gtk::AccessibleAnnouncementPriority::Medium);
        }
    }
}

/// The floating mic button with its state word: bottom right on every page but Overview.
pub struct MicFab {
    root: gtk::Box,
    mic: MicButton,
    pill: gtk::Label,
}

impl MicFab {
    /// Builds the floating button.
    #[must_use]
    pub fn new(controller: &Controller) -> Self {
        let mic = MicButton::new(controller, true);
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        root.set_halign(gtk::Align::End);
        root.set_valign(gtk::Align::End);
        root.set_margin_end(28);
        root.set_margin_bottom(20);
        let pill = label("", &["fab-pill"]);
        pill.set_valign(gtk::Align::Center);
        root.append(&pill);
        root.append(&mic.button);
        let fab = Self { root, mic, pill };
        fab.refresh(&VoiceView::unavailable());
        fab
    }

    /// The widget to overlay.
    #[must_use]
    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    /// Presses the button as a click would (the Ctrl+Space shortcut).
    pub fn press(&self) {
        self.mic.button.emit_clicked();
    }

    /// Redraws from the voice state.
    pub fn refresh(&self, v: &VoiceView) {
        self.mic.refresh(v);
        self.pill.set_text(v.label());
        // The state word is shown only when something is happening or wrong, to stay calm.
        self.pill
            .set_visible(!matches!(v.state, VoiceState::Idle | VoiceState::FollowUp));
    }

    /// Shows or hides the whole button (hidden on Overview, which has the voice card).
    pub fn set_visible(&self, visible: bool) {
        self.root.set_visible(visible);
    }
}

/// The big gradient voice card for Overview: the mic, its state in words, a level meter.
pub struct VoiceCard {
    root: gtk::Box,
    mic: MicButton,
    state_label: gtk::Label,
    hint: gtk::Label,
    level: gtk::ProgressBar,
}

impl VoiceCard {
    /// Builds the card.
    #[must_use]
    pub fn new(controller: &Controller) -> Self {
        let mic = MicButton::new(controller, false);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
        root.add_css_class("gradient-card");
        root.append(&label("VOICE", &["title-card"]));
        let halo = gtk::Box::new(gtk::Orientation::Vertical, 0);
        halo.add_css_class("mic-halo");
        halo.set_halign(gtk::Align::Center);
        halo.set_valign(gtk::Align::Center);
        halo.append(&mic.button);
        halo.set_vexpand(true);
        root.append(&halo);
        let state_label = label("", &["voice-label"]);
        state_label.set_halign(gtk::Align::Center);
        state_label.set_xalign(0.5);
        state_label.set_wrap(true);
        state_label.set_justify(gtk::Justification::Center);
        root.append(&state_label);
        let level = bar();
        level.set_margin_start(12);
        level.set_margin_end(12);
        level.update_property(&[gtk::accessible::Property::Label("Microphone level")]);
        root.append(&level);
        let hint = label("", &["caption"]);
        hint.set_wrap(true);
        hint.set_xalign(0.5);
        hint.set_justify(gtk::Justification::Center);
        root.append(&hint);
        let card = Self {
            root,
            mic,
            state_label,
            hint,
            level,
        };
        card.refresh(&VoiceView::unavailable());
        card
    }

    /// The card's root widget.
    #[must_use]
    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    /// Redraws from the voice state.
    pub fn refresh(&self, v: &VoiceView) {
        self.mic.refresh(v);
        self.state_label.set_text(v.label());
        set_bar(&self.level, f64::from(v.level));
        self.level.set_visible(matches!(
            v.state,
            VoiceState::Listening | VoiceState::Idle | VoiceState::FollowUp
        ));
        self.hint.set_text(hint_for(v.state));
    }
}
