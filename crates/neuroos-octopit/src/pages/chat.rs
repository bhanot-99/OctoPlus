//! The Chat page: conversation view and composer (docs/design/octopit-ux.md §4.2).

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;

use neuroos_octopit_core::chat::{AnswerInfo, Turn, TurnState};
use neuroos_octopit_core::controller::Command;
use neuroos_octopit_core::state::{AppState, Changed};

use super::{Ctx, Page};
use crate::widgets::{label, plain_text};

/// Distance in pixels from the bottom of the scrolled window within which
/// new incoming text automatically scrolls to the bottom.
pub const STICK_THRESHOLD_PX: f64 = 48.0;

/// Returns true if the scroll position is within [`STICK_THRESHOLD_PX`] of the bottom.
#[must_use]
pub fn should_stick_to_bottom(value: f64, page_size: f64, upper: f64) -> bool {
    let distance_from_bottom = upper - (value + page_size);
    distance_from_bottom <= STICK_THRESHOLD_PX
}

/// Returns true if the typed text contains non-whitespace characters.
#[must_use]
pub fn has_sendable_text(raw: &str) -> bool {
    !raw.trim().is_empty()
}

/// Returns a human-readable tooltip for a status chip.
#[must_use]
pub fn chip_tooltip(name: &str) -> &'static str {
    match name {
        "EXTERNAL TEXT" => "This answer contains information retrieved from external text.",
        "WEB" => "A web search was performed to answer this question.",
        "STATUS ONLY" => "The system is reporting component status rather than a generated answer.",
        "PARTIAL DATA" => "Some system data was unavailable when generating this answer.",
        _ => "Answer pipeline route.",
    }
}

/// Returns a human-readable tooltip describing the pipeline route.
#[must_use]
pub fn route_tooltip(route: &str) -> String {
    format!("Answer routed through the {route} pipeline.")
}

struct TurnWidgets {
    root: gtk::Box,
    assistant_bubble: gtk::Box,
    answer_label: gtk::Label,
    thinking_label: gtk::Label,
    status_label: gtk::Label,
    chips_box: gtk::Box,
    chips_built: bool,
    announced: bool,
}

impl TurnWidgets {
    fn update(&mut self, turn: &Turn) {
        let answer_text = turn.answer();
        match &turn.state {
            TurnState::Streaming => {
                if answer_text.is_empty() {
                    self.thinking_label.set_visible(true);
                    self.answer_label.set_visible(false);
                } else {
                    self.thinking_label.set_visible(false);
                    self.answer_label.set_visible(true);
                    if self.answer_label.text() != answer_text {
                        self.answer_label.set_text(&answer_text);
                    }
                }
                self.status_label.set_visible(false);
                self.chips_box.set_visible(false);
            }
            TurnState::Done(info) => {
                self.thinking_label.set_visible(false);
                self.status_label.set_visible(false);
                self.answer_label.set_visible(true);
                if self.answer_label.text() != answer_text {
                    self.answer_label.set_text(&answer_text);
                }
                if !self.chips_built {
                    self.build_chips(info);
                    self.chips_built = true;
                }
                self.chips_box.set_visible(true);
                if !self.announced {
                    self.answer_label
                        .announce("Answer ready", gtk::AccessibleAnnouncementPriority::Medium);
                    self.announced = true;
                }
            }
            TurnState::Cancelled => {
                self.thinking_label.set_visible(false);
                if answer_text.is_empty() {
                    self.answer_label.set_visible(false);
                } else {
                    self.answer_label.set_visible(true);
                    if self.answer_label.text() != answer_text {
                        self.answer_label.set_text(&answer_text);
                    }
                }
                self.status_label.set_text("Stopped.");
                self.status_label.set_visible(true);
                self.chips_box.set_visible(false);
            }
            TurnState::Failed(why) => {
                self.thinking_label.set_visible(false);
                self.assistant_bubble.add_css_class("failed");
                if answer_text.is_empty() {
                    self.answer_label.set_visible(false);
                } else {
                    self.answer_label.set_visible(true);
                    if self.answer_label.text() != answer_text {
                        self.answer_label.set_text(&answer_text);
                    }
                }
                self.status_label.set_text(why);
                self.status_label.set_visible(true);
                self.chips_box.set_visible(false);
            }
        }
    }

    fn build_chips(&self, info: &AnswerInfo) {
        while let Some(child) = self.chips_box.first_child() {
            self.chips_box.remove(&child);
        }

        if info.external {
            self.assistant_bubble.add_css_class("external");
            let chip = label("EXTERNAL TEXT", &["chip", "external"]);
            chip.set_tooltip_text(Some(chip_tooltip("EXTERNAL TEXT")));
            self.chips_box.append(&chip);
        }

        if info.used_web {
            let chip = label("WEB", &["chip", "web"]);
            chip.set_tooltip_text(Some(chip_tooltip("WEB")));
            self.chips_box.append(&chip);
        }

        if info.status_only {
            let chip = label("STATUS ONLY", &["chip", "warn"]);
            chip.set_tooltip_text(Some(chip_tooltip("STATUS ONLY")));
            self.chips_box.append(&chip);
        }

        if info.degraded {
            let chip = label("PARTIAL DATA", &["chip", "warn"]);
            chip.set_tooltip_text(Some(chip_tooltip("PARTIAL DATA")));
            self.chips_box.append(&chip);
        }

        let route_lbl = label(info.route, &["caption", "muted"]);
        let route_tip = route_tooltip(info.route);
        route_lbl.set_tooltip_text(Some(&route_tip));
        self.chips_box.append(&route_lbl);
    }
}

fn update_send_button(button: &gtk::Button, is_busy: bool, has_text: bool) {
    if is_busy {
        button.set_child(Some(&crate::widgets::icon("octopit-stop-symbolic", 16)));
        button.update_property(&[gtk::accessible::Property::Label("Stop answer")]);
        button.set_tooltip_text(Some("Stop answer"));
        button.set_sensitive(true);
    } else {
        button.set_child(Some(&crate::widgets::icon("octopit-send-symbolic", 16)));
        button.update_property(&[gtk::accessible::Property::Label("Send question")]);
        button.set_tooltip_text(Some("Send question"));
        button.set_sensitive(has_text);
    }
}

struct ChatPageInner {
    root: gtk::Box,
    vadjustment: gtk::Adjustment,
    conv_box: gtk::Box,
    empty_state: gtk::Box,
    text_view: gtk::TextView,
    send_button: gtk::Button,
    turns_widgets: RefCell<Vec<TurnWidgets>>,
    ctx: Ctx,
}

impl ChatPageInner {
    fn send(&self, text: &str) {
        let q = {
            let mut st = self.ctx.state.borrow_mut();
            st.conversation.begin(text)
        };
        if let Some(q) = q {
            self.text_view.buffer().set_text("");
            self.sync_turns(&self.ctx.state.borrow());
            if !self.ctx.controller.send(Command::Ask(q)) {
                (self.ctx.toast)("The background service stopped. Restart OctoPit.");
            }
        }
    }

    fn stop(&self) {
        if !self.ctx.controller.send(Command::StopAsk) {
            (self.ctx.toast)("The background service stopped. Restart OctoPit.");
        }
    }

    fn sync_turns(&self, state: &AppState) {
        let turns = state.conversation.turns();
        if turns.is_empty() {
            self.empty_state.set_visible(true);
            for tw in self.turns_widgets.borrow_mut().drain(..) {
                self.conv_box.remove(&tw.root);
            }
            let buf = self.text_view.buffer();
            let (start, end) = buf.bounds();
            let text = buf.text(&start, &end, false);
            update_send_button(&self.send_button, false, has_sendable_text(&text));
            return;
        }

        self.empty_state.set_visible(false);

        let stick = should_stick_to_bottom(
            self.vadjustment.value(),
            self.vadjustment.page_size(),
            self.vadjustment.upper(),
        );

        let mut widgets = self.turns_widgets.borrow_mut();
        let current_len = widgets.len();

        for turn in turns.iter().skip(current_len) {
            let tw = Self::create_turn_widgets(turn);
            self.conv_box.append(&tw.root);
            widgets.push(tw);
        }

        for (turn, widget) in turns.iter().zip(widgets.iter_mut()).rev().take(2) {
            widget.update(turn);
        }

        drop(widgets);

        if stick {
            let vadj = self.vadjustment.clone();
            glib::idle_add_local_once(move || {
                let target = (vadj.upper() - vadj.page_size()).max(0.0);
                vadj.set_value(target);
            });
        }

        let buf = self.text_view.buffer();
        let (start, end) = buf.bounds();
        let text = buf.text(&start, &end, false);
        update_send_button(
            &self.send_button,
            state.conversation.busy(),
            has_sendable_text(&text),
        );
    }

    fn create_turn_widgets(turn: &Turn) -> TurnWidgets {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);

        // User question bubble
        let user_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        user_row.set_halign(gtk::Align::End);
        user_row.set_hexpand(true);

        let user_bubble = gtk::Box::new(gtk::Orientation::Vertical, 0);
        user_bubble.add_css_class("bubble");
        user_bubble.add_css_class("user");
        let user_label = plain_text(&turn.question);
        user_bubble.append(&user_label);
        user_row.append(&user_bubble);
        root.append(&user_row);

        // Assistant answer bubble
        let assistant_row = gtk::Box::new(gtk::Orientation::Vertical, 4);
        assistant_row.set_halign(gtk::Align::Start);
        assistant_row.set_hexpand(true);

        let assistant_bubble = gtk::Box::new(gtk::Orientation::Vertical, 4);
        assistant_bubble.add_css_class("bubble");
        assistant_bubble.add_css_class("assistant");

        let thinking_label = label("Thinking\u{2026}", &["caption", "muted"]);
        thinking_label.set_visible(false);
        assistant_bubble.append(&thinking_label);

        let answer_label = plain_text("");
        answer_label.set_visible(false);
        assistant_bubble.append(&answer_label);

        let status_label = label("", &["caption", "muted"]);
        status_label.set_visible(false);
        assistant_bubble.append(&status_label);

        assistant_row.append(&assistant_bubble);

        let chips_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        chips_box.set_margin_top(4);
        chips_box.set_margin_start(4);
        chips_box.set_visible(false);
        assistant_row.append(&chips_box);

        root.append(&assistant_row);

        let mut tw = TurnWidgets {
            root,
            assistant_bubble,
            answer_label,
            thinking_label,
            status_label,
            chips_box,
            chips_built: false,
            announced: false,
        };

        tw.update(turn);
        tw
    }
}

/// The Chat page.
pub struct ChatPage {
    inner: Rc<ChatPageInner>,
}

impl ChatPage {
    /// Builds the page.
    #[must_use]
    pub fn new(ctx: &Ctx) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
        root.set_margin_top(16);

        // Header
        let header_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        header_box.set_margin_start(24);
        header_box.set_margin_end(24);
        header_box.append(&label("Chat", &["title-page"]));
        header_box.append(&label(
            "Typed questions only. Spoken conversations are not shown here yet.",
            &["caption"],
        ));
        root.append(&header_box);

        // Scrolled conversation area
        let scrolled = gtk::ScrolledWindow::new();
        scrolled.set_vexpand(true);
        scrolled.set_hexpand(true);
        scrolled.set_hscrollbar_policy(gtk::PolicyType::Automatic);
        let vadjustment = scrolled.vadjustment();

        let conv_box = gtk::Box::new(gtk::Orientation::Vertical, 16);
        conv_box.set_margin_start(24);
        conv_box.set_margin_end(24);
        conv_box.set_margin_top(16);
        conv_box.set_margin_bottom(16);
        conv_box.add_css_class("chat-column");
        conv_box.set_accessible_role(gtk::AccessibleRole::Log);
        conv_box.update_property(&[gtk::accessible::Property::Label("Conversation")]);
        scrolled.set_child(Some(&conv_box));
        root.append(&scrolled);

        // Empty state
        let empty_state = gtk::Box::new(gtk::Orientation::Vertical, 12);
        empty_state.add_css_class("chat-empty");
        empty_state.set_halign(gtk::Align::Center);
        empty_state.set_valign(gtk::Align::Center);
        empty_state.set_vexpand(true);

        let heading = label("Ask NeuroOS anything", &["title-section"]);
        heading.set_halign(gtk::Align::Center);
        heading.set_xalign(0.5);
        empty_state.append(&heading);

        let sub = label("Everything is answered on this machine.", &["caption"]);
        sub.set_halign(gtk::Align::Center);
        sub.set_xalign(0.5);
        empty_state.append(&sub);

        let prompts = gtk::Box::new(gtk::Orientation::Vertical, 8);
        prompts.add_css_class("chat-example-box");
        empty_state.append(&prompts);
        conv_box.append(&empty_state);

        // Composer
        let composer_container = gtk::Box::new(gtk::Orientation::Vertical, 6);
        composer_container.set_margin_start(24);
        composer_container.set_margin_end(24);
        composer_container.set_margin_bottom(16);

        let composer_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        composer_box.add_css_class("composer");

        let text_view = gtk::TextView::new();
        text_view.set_wrap_mode(gtk::WrapMode::WordChar);
        text_view.set_hexpand(true);
        text_view.set_accepts_tab(false);
        text_view.set_left_margin(4);
        text_view.set_right_margin(4);
        text_view.set_top_margin(4);
        text_view.set_bottom_margin(4);
        text_view.update_property(&[gtk::accessible::Property::Label("Question input")]);
        composer_box.append(&text_view);

        let send_button = gtk::Button::new();
        send_button.add_css_class("btn");
        send_button.add_css_class("primary");
        send_button.set_valign(gtk::Align::End);
        update_send_button(&send_button, false, false);
        composer_box.append(&send_button);

        composer_container.append(&composer_box);
        let hint = label(
            "Enter to send \u{00B7} Shift+Enter for a new line \u{00B7} Esc to stop",
            &["caption", "muted"],
        );
        hint.set_halign(gtk::Align::Center);
        hint.set_xalign(0.5);
        composer_container.append(&hint);
        root.append(&composer_container);

        let inner = Rc::new(ChatPageInner {
            root,
            vadjustment,
            conv_box,
            empty_state,
            text_view: text_view.clone(),
            send_button: send_button.clone(),
            turns_widgets: RefCell::new(Vec::new()),
            ctx: ctx.clone(),
        });

        // Setup example prompt buttons
        for text in [
            "What can you do?",
            "How much battery is left?",
            "What was I working on today?",
        ] {
            let btn = gtk::Button::with_label(text);
            btn.add_css_class("example-prompt");
            btn.update_property(&[gtk::accessible::Property::Label(text)]);
            let inner_weak = Rc::downgrade(&inner);
            let prompt_text = text.to_owned();
            btn.connect_clicked(move |_| {
                if let Some(inner) = inner_weak.upgrade() {
                    inner.send(&prompt_text);
                }
            });
            prompts.append(&btn);
        }

        // Setup key controller on TextView
        let key_ctrl = gtk::EventControllerKey::new();
        key_ctrl.set_propagation_phase(gtk::PropagationPhase::Capture);
        let inner_weak = Rc::downgrade(&inner);
        key_ctrl.connect_key_pressed(move |_, key, _keycode, state| {
            let Some(inner) = inner_weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if key == gdk::Key::Escape {
                let is_busy = inner.ctx.state.borrow().conversation.busy();
                if is_busy {
                    inner.stop();
                    return glib::Propagation::Stop;
                }
            } else if key == gdk::Key::Return || key == gdk::Key::KP_Enter {
                if state.contains(gdk::ModifierType::SHIFT_MASK) {
                    return glib::Propagation::Proceed;
                }
                let buf = inner.text_view.buffer();
                let (start, end) = buf.bounds();
                let text = buf.text(&start, &end, false);
                if has_sendable_text(&text) {
                    inner.send(&text);
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        text_view.add_controller(key_ctrl);

        // Buffer changed listener
        let send_btn_clone = send_button.clone();
        let state_clone = ctx.state.clone();
        text_view.buffer().connect_changed(move |buf| {
            let (start, end) = buf.bounds();
            let text = buf.text(&start, &end, false);
            let is_busy = state_clone.borrow().conversation.busy();
            update_send_button(&send_btn_clone, is_busy, has_sendable_text(&text));
        });

        // Send button clicked listener
        let inner_weak = Rc::downgrade(&inner);
        send_button.connect_clicked(move |_| {
            let Some(inner) = inner_weak.upgrade() else {
                return;
            };
            let is_busy = inner.ctx.state.borrow().conversation.busy();
            if is_busy {
                inner.stop();
            } else {
                let buf = inner.text_view.buffer();
                let (start, end) = buf.bounds();
                let text = buf.text(&start, &end, false);
                if has_sendable_text(&text) {
                    inner.send(&text);
                }
            }
        });

        Self { inner }
    }
}

impl Page for ChatPage {
    fn widget(&self) -> gtk::Widget {
        self.inner.root.clone().upcast()
    }

    fn refresh(&self, state: &AppState, changed: Changed) {
        if changed.chat {
            self.inner.sync_turns(state);
        }
    }

    fn shown(&self) {
        self.inner.text_view.grab_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stick_to_bottom_calculation() {
        // Exactly at bottom
        assert!(should_stick_to_bottom(500.0, 500.0, 1000.0));
        // Within 48 px threshold
        assert!(should_stick_to_bottom(460.0, 500.0, 1000.0));
        assert!(should_stick_to_bottom(452.0, 500.0, 1000.0));
        // Beyond 48 px threshold (user scrolled up)
        assert!(!should_stick_to_bottom(451.0, 500.0, 1000.0));
        assert!(!should_stick_to_bottom(200.0, 500.0, 1000.0));
        assert!(!should_stick_to_bottom(0.0, 500.0, 1000.0));
        // Overscrolled past bottom
        assert!(should_stick_to_bottom(510.0, 500.0, 1000.0));
    }

    #[test]
    fn test_has_sendable_text() {
        assert!(!has_sendable_text(""));
        assert!(!has_sendable_text("   "));
        assert!(!has_sendable_text("\n\t  \r"));
        assert!(has_sendable_text("hello"));
        assert!(has_sendable_text("  hello world  "));
        assert!(has_sendable_text("?"));
    }

    #[test]
    fn test_chip_tooltips() {
        assert!(!chip_tooltip("EXTERNAL TEXT").is_empty());
        assert!(!chip_tooltip("WEB").is_empty());
        assert!(!chip_tooltip("STATUS ONLY").is_empty());
        assert!(!chip_tooltip("PARTIAL DATA").is_empty());
        assert!(!chip_tooltip("OTHER").is_empty());

        let route_tip = route_tooltip("action");
        assert!(route_tip.contains("action"));
    }
}
