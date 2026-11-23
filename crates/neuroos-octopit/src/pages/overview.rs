//! Overview: is everything healthy, and what is each part doing (docs/design/octopit-ux.md §4.1).

use std::cell::RefCell;

use gtk::prelude::*;
use gtk4 as gtk;

use neuroos_common::paths::Part;
use neuroos_octopit_core::format;
use neuroos_octopit_core::health::{Headline, PartCard, PartState};
use neuroos_octopit_core::state::{AppState, Changed};

use super::{Ctx, Page, PageId};
use crate::widgets::{bar, card, dot, label, set_bar, set_dot, state_class, state_word};

struct Tile {
    root: gtk::Box,
    dot: gtk::Box,
    state: gtk::Label,
    bar: gtk::ProgressBar,
    mem: gtk::Label,
    speed: gtk::Label,
}

/// The Overview page.
pub struct OverviewPage {
    root: gtk::ScrolledWindow,
    hero: gtk::Box,
    hero_dot: gtk::Box,
    hero_title: gtk::Label,
    hero_sub: gtk::Label,
    hero_button: gtk::Button,
    tiles: Vec<(Part, Tile)>,
    key_speed: gtk::Label,
    key_memory: gtk::Label,
    key_egress: gtk::Label,
    key_gpu: gtk::Label,
    attention: RefCell<Option<Part>>,
}

fn key_number(title: &str, value: &gtk::Label) -> gtk::Box {
    let b = card(&["raised"]);
    b.set_hexpand(true);
    b.append(&label(title, &["title-card"]));
    value.add_css_class("metric");
    value.add_css_class("num");
    b.append(value);
    b
}

fn tile(card_data: &PartCard) -> Tile {
    let root = card(&[]);
    root.set_hexpand(true);
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let d = dot(card_data.state);
    head.append(&d);
    head.append(&label(card_data.title, &["title-section"]));
    root.append(&head);
    let role = label(card_data.role, &["caption"]);
    role.set_wrap(true);
    role.set_max_width_chars(30);
    root.set_size_request(190, -1);
    root.append(&role);
    let state = label("", &["caption"]);
    root.append(&state);
    let b = bar();
    root.append(&b);
    let mem = label("", &["caption", "num"]);
    root.append(&mem);
    let speed = label("", &["caption", "num"]);
    root.append(&speed);
    Tile {
        root,
        dot: d,
        state,
        bar: b,
        mem,
        speed,
    }
}

impl OverviewPage {
    /// Builds the page.
    #[must_use]
    pub fn new(ctx: &Ctx) -> Self {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
        content.add_css_class("page");

        content.append(&label("Overview", &["title-page"]));

        // Hero.
        let hero = card(&["hero", "raised"]);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        let hero_dot = dot(PartState::Starting);
        hero_dot.set_size_request(16, 16);
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
        texts.set_hexpand(true);
        let hero_title = label("Connecting\u{2026}", &["title-section"]);
        let hero_sub = label("", &["caption"]);
        texts.append(&hero_title);
        texts.append(&hero_sub);
        let hero_button = gtk::Button::with_label("See details");
        hero_button.add_css_class("btn");
        hero_button.set_valign(gtk::Align::Center);
        hero_button.set_visible(false);
        row.append(&hero_dot);
        row.append(&texts);
        row.append(&hero_button);
        hero.append(&row);
        content.append(&hero);

        // Key numbers.
        let key_speed = gtk::Label::new(Some("\u{2014}"));
        let key_memory = gtk::Label::new(Some("\u{2014}"));
        let key_egress = gtk::Label::new(Some("\u{2014}"));
        let key_gpu = gtk::Label::new(Some("\u{2014}"));
        for l in [&key_speed, &key_memory, &key_egress, &key_gpu] {
            l.set_xalign(0.0);
        }
        let keys = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        keys.set_homogeneous(true);
        keys.append(&key_number("MODEL SPEED", &key_speed));
        keys.append(&key_number("MEMORY IN USE", &key_memory));
        keys.append(&key_number("SENT TO NETWORK", &key_egress));
        keys.append(&key_number("GPU", &key_gpu));
        content.append(&keys);

        // Parts grid.
        content.append(&label("Parts", &["title-section"]));
        let flow = gtk::FlowBox::new();
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_min_children_per_line(1);
        flow.set_max_children_per_line(4);
        flow.set_column_spacing(12);
        flow.set_row_spacing(12);
        flow.set_homogeneous(true);
        flow.set_activate_on_single_click(false);
        let initial = AppState::new();
        let mut tiles = Vec::new();
        for c in &initial.snapshot.parts {
            let t = tile(c);
            flow.append(&t.root);
            tiles.push((c.part, t));
        }
        content.append(&flow);

        let navigate = ctx.navigate.clone();
        let attention: RefCell<Option<Part>> = RefCell::new(None);
        hero_button.connect_clicked(move |_| navigate(PageId::Backends));

        let root = gtk::ScrolledWindow::new();
        root.set_child(Some(&content));
        root.set_hscrollbar_policy(gtk::PolicyType::Never);
        Self {
            root,
            hero,
            hero_dot,
            hero_title,
            hero_sub,
            hero_button,
            tiles,
            key_speed,
            key_memory,
            key_egress,
            key_gpu,
            attention,
        }
    }

    fn redraw_health(&self, st: &AppState) {
        let snap = &st.snapshot;
        let hero_state = if !st.have_snapshot {
            PartState::Starting
        } else {
            match snap.headline() {
                Headline::Healthy => PartState::Ok,
                Headline::Unreachable => PartState::Down,
                Headline::Attention { state, .. } => match state {
                    PartState::Down => PartState::Down,
                    PartState::Ok => PartState::Ok,
                    _ => PartState::Degraded,
                },
            }
        };
        for c in ["ok", "degraded", "down"] {
            self.hero.remove_css_class(c);
        }
        if st.have_snapshot {
            self.hero.add_css_class(state_class(hero_state));
        }
        set_dot(&self.hero_dot, hero_state);
        self.hero_title.set_text(&st.headline_text());
        self.hero_sub.set_text(&if st.have_snapshot {
            format!(
                "{} of {} parts running \u{00B7} {} MiB of {} MiB memory budget",
                snap.healthy, snap.total, snap.total_rss_mib, snap.total_budget_mib
            )
        } else {
            "Reading the state of every part".to_owned()
        });
        *self.attention.borrow_mut() = match snap.headline() {
            Headline::Attention { part, .. } if st.have_snapshot => Some(part),
            _ => None,
        };
        self.hero_button.set_visible(self.attention.borrow().is_some());

        for (part, t) in &self.tiles {
            let Some(c) = snap.parts.iter().find(|p| p.part == *part) else {
                continue;
            };
            set_dot(&t.dot, c.state);
            t.root.remove_css_class("attention");
            t.root.remove_css_class("down");
            match c.state {
                PartState::Degraded => t.root.add_css_class("attention"),
                PartState::Down if st.have_snapshot => t.root.add_css_class("down"),
                _ => {}
            }
            t.state.set_text(&if st.have_snapshot {
                let up = if c.state == PartState::Ok && c.uptime_secs > 0 {
                    format!(" \u{00B7} up {}", format::uptime(c.uptime_secs))
                } else {
                    String::new()
                };
                format!("{}{up}", state_word(c.state))
            } else {
                "\u{2014}".to_owned()
            });
            for cl in ["state-ok", "state-degraded", "state-down", "state-starting"] {
                t.state.remove_css_class(cl);
            }
            t.state.add_css_class(&format!("state-{}", state_class(c.state)));
            set_bar(&t.bar, c.rss_fraction());
            t.mem.set_text(&format!(
                "{} of {} MiB",
                c.rss_bytes / (1024 * 1024),
                c.budget_mib
            ));
            t.speed.set_text(&if c.p99_ns > 0 {
                format!("Slowest 1%: {}", format::latency(c.p99_ns))
            } else {
                String::new()
            });
        }
        // Key numbers.
        let speed = snap
            .parts
            .iter()
            .find(|p| p.part == Part::Inference)
            .map_or(0, |p| p.p99_ns);
        self.key_speed.set_text(&if speed > 0 {
            format!("{} / token", format::latency(speed))
        } else {
            "\u{2014}".to_owned()
        });
        self.key_memory.set_text(&if st.have_snapshot {
            format::bytes_iec(snap.total_rss_mib * 1024 * 1024)
        } else {
            "\u{2014}".to_owned()
        });
    }
}

impl Page for OverviewPage {
    fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    fn refresh(&self, st: &AppState, ch: Changed) {
        if ch.health {
            self.redraw_health(st);
        }
        if ch.ledgers || ch.health {
            self.key_egress.set_text(&match st.ledgers.egress_bytes_out() {
                Some(b) => format::bytes_iec(b),
                None => "\u{2014}".to_owned(),
            });
        }
        if ch.gpu {
            self.key_gpu.set_text(&match &st.gpu {
                Some(neuroos_octopit_core::gpu::GpuState::Present(r)) => r
                    .utilization_pct
                    .map_or_else(|| "\u{2014}".to_owned(), |u| format::percent(u / 100.0)),
                Some(neuroos_octopit_core::gpu::GpuState::NotAvailable) => "None".to_owned(),
                None => "\u{2014}".to_owned(),
            });
        }
    }
}

