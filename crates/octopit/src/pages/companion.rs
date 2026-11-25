//! The Companion page (Phase 12, story C-S06): her rhythm, today's talks, the switches, and
//! what she remembers about the owner. Status and switches only: there is no chat here and
//! nothing that acts on the computer (DR-1). She talks by voice.

use gtk::prelude::*;
use gtk4 as gtk;

use octopit_core::companion::{CompanionCommand, CompanionView};
use octopit_core::controller::{Command, Controller};
use octopit_core::format;
use octopit_core::state::{AppState, Changed};

use super::{Ctx, Page};
use crate::widgets::{card, label};

/// Most memory rows drawn at once.
const MAX_MEMORY_ROWS: usize = 40;
/// Most talk rows drawn at once.
const MAX_TALK_ROWS: usize = 30;

/// The Companion page.
pub struct CompanionPage {
    root: gtk::ScrolledWindow,
    controller: Controller,
    headline: gtk::Label,
    rhythm: gtk::Label,
    pause_button: gtk::Button,
    talks_empty: gtk::Label,
    talks_rows: gtk::Box,
    memory_note: gtk::Label,
    memory_rows: gtk::Box,
}

fn action_button(text: &str, description: &str) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    b.add_css_class("btn");
    b.update_property(&[gtk::accessible::Property::Label(description)]);
    b
}

fn clear(b: &gtk::Box) {
    while let Some(child) = b.first_child() {
        b.remove(&child);
    }
}

impl CompanionPage {
    /// Builds the page.
    #[must_use]
    pub fn new(ctx: &Ctx) -> Self {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
        content.add_css_class("page");
        content.append(&label("Companion", &["title-page"]));
        content.append(&label(
            "Laila talks by voice. This page shows how she is doing and lets you steer her.",
            &["caption"],
        ));

        // Status and switches
        let status = card(&["raised", "hero"]);
        let headline = label("Companion: connecting\u{2026}", &["title-section"]);
        headline.set_wrap(true);
        let rhythm = label("", &["caption"]);
        rhythm.set_wrap(true);
        status.append(&headline);
        status.append(&rhythm);

        let row1 = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row1.set_halign(gtk::Align::Start);
        let pause_button = action_button("Pause", "Pause the companion");
        let more = action_button("Talk more", "Make the companion talk a bit more");
        let less = action_button("Talk less", "Make the companion talk a bit less");
        row1.append(&pause_button);
        row1.append(&more);
        row1.append(&less);
        status.append(&row1);

        let row2 = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row2.set_halign(gtk::Align::Start);
        row2.append(&label("Quiet for", &["caption"]));
        for (text, minutes) in [("30 min", 30u32), ("1 hour", 60), ("4 hours", 240)] {
            let b = action_button(text, &format!("Ask the companion to be quiet for {text}"));
            let c = ctx.controller.clone();
            b.connect_clicked(move |_| {
                let _ = c.send(Command::Companion(CompanionCommand::QuietFor(minutes)));
            });
            row2.append(&b);
        }
        status.append(&row2);

        let row3 = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row3.set_halign(gtk::Align::Start);
        row3.append(&label("Her last joke", &["caption"]));
        let funny = action_button("That was funny", "The last joke was funny");
        let not_funny = action_button("Not funny", "The last joke was not funny");
        row3.append(&funny);
        row3.append(&not_funny);
        status.append(&row3);
        content.append(&status);

        let send = |c: &Controller, cmd: CompanionCommand| {
            let _ = c.send(Command::Companion(cmd));
        };
        {
            let c = ctx.controller.clone();
            let state = ctx.state.clone();
            pause_button.connect_clicked(move |_| {
                let cmd = if state.borrow().companion.shows_resume() {
                    CompanionCommand::Resume
                } else {
                    CompanionCommand::Pause
                };
                send(&c, cmd);
            });
        }
        {
            let c = ctx.controller.clone();
            more.connect_clicked(move |_| send(&c, CompanionCommand::ChatMore));
        }
        {
            let c = ctx.controller.clone();
            less.connect_clicked(move |_| send(&c, CompanionCommand::ChatLess));
        }
        {
            let c = ctx.controller.clone();
            funny.connect_clicked(move |_| send(&c, CompanionCommand::Funny));
        }
        {
            let c = ctx.controller.clone();
            not_funny.connect_clicked(move |_| send(&c, CompanionCommand::NotFunny));
        }

        let flow = gtk::FlowBox::new();
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_min_children_per_line(1);
        flow.set_max_children_per_line(2);
        flow.set_column_spacing(20);
        flow.set_row_spacing(20);
        flow.set_homogeneous(false);
        flow.set_can_focus(false);
        content.append(&flow);

        // Today's talks
        let talks_card = card(&[]);
        talks_card.append(&label("Today's talks", &["title-section"]));
        let talks_empty = label(
            "She has not started a talk today. She waits for a free moment.",
            &["caption", "muted"],
        );
        talks_empty.set_wrap(true);
        talks_card.append(&talks_empty);
        let talks_rows = gtk::Box::new(gtk::Orientation::Vertical, 4);
        talks_card.append(&talks_rows);
        talks_card.set_hexpand(true);
        talks_card.set_valign(gtk::Align::Start);
        talks_card.set_size_request(320, -1);
        flow.append(&talks_card);

        // What she remembers
        let memory_card = card(&[]);
        memory_card.append(&label("What she remembers", &["title-section"]));
        let memory_note = label("", &["caption", "muted"]);
        memory_note.set_wrap(true);
        memory_card.append(&memory_note);
        let memory_rows = gtk::Box::new(gtk::Orientation::Vertical, 4);
        memory_card.append(&memory_rows);
        memory_card.set_hexpand(true);
        memory_card.set_valign(gtk::Align::Start);
        memory_card.set_size_request(320, -1);
        flow.append(&memory_card);

        // Her character
        let persona = card(&[]);
        persona.append(&label("Her character", &["title-section"]));
        let p = label(
            "Her name, her two sides, her quiet hours and the topics she avoids are in ~/.config/octoplus/persona.toml. Edit that file and restart OctoPlus to change them. An editor in this window comes with the next OctoPit.",
            &["caption"],
        );
        p.set_wrap(true);
        persona.append(&p);
        persona.set_hexpand(true);
        persona.set_valign(gtk::Align::Start);
        persona.set_size_request(320, -1);
        flow.append(&persona);

        let root = gtk::ScrolledWindow::new();
        root.set_child(Some(&content));
        root.set_hscrollbar_policy(gtk::PolicyType::Automatic);

        Self {
            root,
            controller: ctx.controller.clone(),
            headline,
            rhythm,
            pause_button,
            talks_empty,
            talks_rows,
            memory_note,
            memory_rows,
        }
    }

    fn redraw(&self, v: &CompanionView) {
        self.headline.set_text(&v.headline());
        self.rhythm.set_text(&v.rhythm_line());
        self.rhythm.set_visible(!v.rhythm_line().is_empty());
        self.pause_button
            .set_label(if v.shows_resume() { "Resume" } else { "Pause" });

        let now = octoplus_common::time::now_ns().unwrap_or(0);
        clear(&self.talks_rows);
        self.talks_empty.set_visible(v.talks.is_empty());
        for t in v.talks.iter().take(MAX_TALK_ROWS) {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            row.add_css_class("row-line");
            row.append(&label(
                &format::age_label(now, t.at_ns),
                &["caption", "num"],
            ));
            let topic = label(&t.topic, &["body"]);
            topic.set_hexpand(true);
            row.append(&topic);
            row.append(&label(&t.reaction, &["caption", "muted"]));
            self.talks_rows.append(&row);
        }

        self.memory_note.set_text(&v.memory_note());
        clear(&self.memory_rows);
        for m in v.memories.iter().take(MAX_MEMORY_ROWS) {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            row.add_css_class("row-line");
            let text = label(&m.text, &["body"]);
            text.set_wrap(true);
            text.set_hexpand(true);
            row.append(&text);
            row.append(&label(m.how(), &["caption", "muted"]));
            let forget = action_button("Forget", "Forget this memory");
            let c = self.controller.clone();
            let id = m.id;
            forget.connect_clicked(move |_| {
                let _ = c.send(Command::Companion(CompanionCommand::Forget(id)));
            });
            row.append(&forget);
            self.memory_rows.append(&row);
        }
    }
}

impl Page for CompanionPage {
    fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    fn refresh(&self, state: &AppState, changed: Changed) {
        if changed.companion {
            self.redraw(&state.companion);
        }
    }
}
