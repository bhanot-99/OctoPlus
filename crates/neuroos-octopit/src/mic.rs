//! The voice dock: the always-visible mic button, its state in words, and the level meter
//! (docs/design/octopit-ux.md §3). It only sends start, stop and cancel; it never touches audio.

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;

use neuroos_octopit_core::controller::{Command, Controller};
use neuroos_octopit_core::voice::{MicAction, VoiceState, VoiceView};

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

/// The dock on the right of the window.
pub struct MicDock {
    root: gtk::Box,
    button: gtk::Button,
    glyph: gtk::Image,
    state_label: gtk::Label,
    hint: gtk::Label,
    level: gtk::ProgressBar,
    action: Rc<Cell<MicAction>>,
    last_state: Cell<Option<VoiceState>>,
}

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

impl MicDock {
    /// Builds the dock.
    #[must_use]
    pub fn new(controller: &Controller) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 14);
        root.add_css_class("voice-dock");
        root.set_size_request(232, -1);
        root.set_halign(gtk::Align::Fill);

        root.append(&label("VOICE", &["title-card"]));

        let halo = gtk::Box::new(gtk::Orientation::Vertical, 0);
        halo.add_css_class("mic-halo");
        halo.set_halign(gtk::Align::Center);
        halo.set_valign(gtk::Align::Center);
        let button = gtk::Button::new();
        button.add_css_class("mic-button");
        button.set_halign(gtk::Align::Center);
        button.set_valign(gtk::Align::Center);
        button.set_tooltip_text(Some("Talk to NeuroOS (Ctrl+Space)"));
        let glyph = icon("octopit-mic-symbolic", 32);
        button.set_child(Some(&glyph));
        halo.append(&button);
        root.append(&halo);

        let state_label = label("", &["voice-label"]);
        state_label.set_halign(gtk::Align::Center);
        state_label.set_xalign(0.5);
        state_label.set_wrap(true);
        state_label.set_justify(gtk::Justification::Center);
        root.append(&state_label);

        let level = bar();
        level.set_margin_start(8);
        level.set_margin_end(8);
        level.update_property(&[gtk::accessible::Property::Label("Microphone level")]);
        root.append(&level);

        let hint = label("", &["caption"]);
        hint.set_wrap(true);
        hint.set_xalign(0.5);
        hint.set_justify(gtk::Justification::Center);
        root.append(&hint);

        let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        spacer.set_vexpand(true);
        root.append(&spacer);
        let privacy = label(
            "Audio is processed on this machine and never stored. OctoPit never hears it.",
            &["caption", "muted"],
        );
        privacy.set_wrap(true);
        privacy.set_xalign(0.0);
        root.append(&privacy);

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
        let dock = Self {
            root,
            button,
            glyph,
            state_label,
            hint,
            level,
            action,
            last_state: Cell::new(None),
        };
        dock.refresh(&VoiceView::unavailable());
        dock
    }

    /// The dock's root widget.
    #[must_use]
    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    /// Presses the button as a click would (the Ctrl+Space shortcut).
    pub fn press(&self) {
        self.button.emit_clicked();
    }

    /// Redraws from the voice state.
    pub fn refresh(&self, v: &VoiceView) {
        self.action.set(v.press());
        for c in STATE_CLASSES {
            self.button.remove_css_class(c);
        }
        self.button.add_css_class(class_for(v.state));
        self.button.set_sensitive(v.press() != MicAction::None);
        self.glyph.set_icon_name(Some(glyph_for(v)));
        self.state_label.set_text(v.label());
        self.button
            .update_property(&[gtk::accessible::Property::Label(v.button_label())]);
        self.button.set_tooltip_text(Some(v.button_label()));
        set_bar(&self.level, f64::from(v.level));
        self.level
            .set_visible(matches!(v.state, VoiceState::Listening | VoiceState::Idle | VoiceState::FollowUp));
        self.hint.set_text(match v.state {
            VoiceState::Idle | VoiceState::FollowUp => "Wake phrase: \u{201c}hey jarvis\u{201d}\nCtrl+Space to talk",
            VoiceState::Listening => "Press again to finish",
            VoiceState::Thinking | VoiceState::Speaking => "Press to stop",
            VoiceState::Busy => "Close the other app, then try again",
            VoiceState::Off => "Start it with: systemctl --user start neuroos-voice",
            VoiceState::Unavailable => "Waiting for the voice part",
        });
        // Tell screen readers when the state changes (never on every level update).
        if self.last_state.get() != Some(v.state) {
            self.last_state.set(Some(v.state));
            self.state_label.announce(v.label(), gtk::AccessibleAnnouncementPriority::Medium);
        }
    }
}
