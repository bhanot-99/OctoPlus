//! Overview page in the v2 visual language (docs/design/octopit-ux.md §10).

use std::cell::Cell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;

use neuroos_common::paths::Part;
use neuroos_octopit_core::format;
use neuroos_octopit_core::health::{Headline, PartCard, PartState, Snapshot};
use neuroos_octopit_core::state::{AppState, Changed};

use super::{Ctx, Page, PageId};
use crate::charts::{Bar, BarChart, Donut, Orb, OrbTint, series_css_class};
use crate::mic::VoiceCard;
use crate::widgets::{
    bar, card, dot, icon, label, set_bar, set_dot, skeleton, state_class, state_word,
};

/// Mode for the Overview bar chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BarChartMode {
    /// Memory used as fraction of budget.
    #[default]
    Memory,
    /// Slowest 1% latency relative to the slowest part.
    Speed,
}

/// Returns a friendly time-based greeting for the hour (0..=23).
#[must_use]
pub fn greeting(hour: i32) -> &'static str {
    match hour {
        5..=11 => "Good morning.",
        12..=16 => "Good afternoon.",
        _ => "Good evening.",
    }
}

/// Combines greeting and headline into the sub-header sentence.
#[must_use]
pub fn header_sentence(hour: i32, headline: &str) -> String {
    let greet = greeting(hour);
    let trimmed = headline.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        greet.to_owned()
    } else {
        format!("{greet} {trimmed}.")
    }
}

/// Returns the short label for a part in the overview bar chart.
#[must_use]
pub fn part_short_label(part: Part) -> &'static str {
    match part {
        Part::Healthd => "Health",
        Part::Storage => "Memory",
        Part::Inference => "Model",
        Part::Monitor => "Sensor",
        Part::Kernel => "Kernel",
        Part::Knowledge => "Assistant",
        Part::Voice => "Voice",
        Part::Fetcher => "Fetcher",
    }
}

/// Computes memory usage fractions (0.0..=1.0) for each part in `Part::ALL` order.
#[must_use]
pub fn memory_bar_fractions(cards: &[PartCard]) -> Vec<f64> {
    Part::ALL
        .iter()
        .map(|&part| {
            cards
                .iter()
                .find(|c| c.part == part)
                .map_or(0.0, |c| c.rss_fraction().clamp(0.0, 1.0))
        })
        .collect()
}

/// Computes p99 latency fractions relative to the slowest part in `Part::ALL` order.
#[must_use]
pub fn speed_bar_fractions(cards: &[PartCard]) -> Vec<f64> {
    let max_p99 = cards.iter().map(|c| c.p99_ns).max().unwrap_or(0);
    #[allow(clippy::cast_precision_loss)]
    let max_f = max_p99 as f64;
    Part::ALL
        .iter()
        .map(|&part| {
            if max_p99 == 0 {
                0.0
            } else {
                let p99 = cards
                    .iter()
                    .find(|c| c.part == part)
                    .map_or(0, |c| c.p99_ns);
                #[allow(clippy::cast_precision_loss)]
                let f = p99 as f64 / max_f;
                f.clamp(0.0, 1.0)
            }
        })
        .collect()
}

/// Computes (spent_bytes, available_bytes) with saturation against total budget.
#[must_use]
pub fn memory_spent_available(total_rss_bytes: u64, total_budget_mib: u64) -> (u64, u64) {
    let budget_bytes = total_budget_mib.saturating_mul(1024 * 1024);
    let available = budget_bytes.saturating_sub(total_rss_bytes);
    (total_rss_bytes, available)
}

/// Returns the chip CSS modifier class for an audit risk tier.
#[must_use]
pub fn audit_tier_class(tier: &str) -> &'static str {
    match tier {
        "SAFE" => "ok",
        "REVIEW" => "warn",
        "DANGEROUS" => "down",
        _ => "",
    }
}

/// Determines the orb tint from the health headline and connection state.
#[must_use]
pub fn orb_tint(headline: &Headline, have_snapshot: bool) -> OrbTint {
    if !have_snapshot {
        OrbTint::Healthy
    } else {
        match headline {
            Headline::Healthy => OrbTint::Healthy,
            Headline::Unreachable => OrbTint::Down,
            Headline::Attention { state, .. } => {
                if *state == PartState::Down {
                    OrbTint::Down
                } else {
                    OrbTint::Attention
                }
            }
        }
    }
}

struct PartTileWidgets {
    dot: gtk::Box,
    state_label: gtk::Label,
    mem_label: gtk::Label,
}

/// The Overview page in the v2 visual language.
pub struct OverviewPage {
    root: gtk::ScrolledWindow,
    greet_label: gtk::Label,

    // Card 1: Health
    orb: Orb,
    health_headline: gtk::Label,
    health_caption: gtk::Label,
    bar_chart: BarChart,
    bar_caption: gtk::Label,
    chart_mode: Rc<Cell<BarChartMode>>,

    // Card 2: Memory budget
    spent_val: gtk::Label,
    avail_val: gtk::Label,
    budget_bar: gtk::ProgressBar,
    budget_caption: gtk::Label,
    donut: Donut,
    donut_total_val: gtk::Label,
    legend_box: gtk::FlowBox,

    // Card 3: Voice card
    voice_card: VoiceCard,

    // Card B: Last activity
    audit_box: gtk::Box,

    // Parts grid
    part_tiles: Vec<(Part, PartTileWidgets)>,
}

impl OverviewPage {
    /// Builds the Overview page.
    #[must_use]
    pub fn new(ctx: &Ctx) -> Self {
        let root = gtk::ScrolledWindow::new();
        root.set_hscrollbar_policy(gtk::PolicyType::Automatic);
        root.set_vscrollbar_policy(gtk::PolicyType::Automatic);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 20);
        content.add_css_class("page");

        // Header block
        let header_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let title_lbl = label("Overview", &["title-page"]);
        let hour = glib::DateTime::now_local().map_or(12, |dt| dt.hour());
        let greet_label = label(
            &header_sentence(hour, "Connecting\u{2026}"),
            &["title-greet"],
        );
        greet_label.set_wrap(true);
        header_box.append(&title_lbl);
        header_box.append(&greet_label);
        content.append(&header_box);

        // Row 1: Three-column FlowBox
        let flow1 = gtk::FlowBox::new();
        flow1.set_selection_mode(gtk::SelectionMode::None);
        flow1.set_min_children_per_line(1);
        flow1.set_max_children_per_line(3);
        flow1.set_homogeneous(false);
        flow1.set_column_spacing(20);
        flow1.set_row_spacing(20);

        // -------------------------------------------------------------
        // Card 1: Health Card
        // -------------------------------------------------------------
        let card1 = card(&["raised"]);
        card1.set_hexpand(true);
        card1.set_size_request(300, -1);

        card1.append(&label("SYSTEM HEALTH", &["title-card"]));

        let orb = Orb::new(132);
        let orb_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        orb_box.set_halign(gtk::Align::Center);
        orb_box.append(orb.widget());
        card1.append(&orb_box);

        let health_headline = label("Connecting\u{2026}", &["metric-md"]);
        health_headline.set_halign(gtk::Align::Center);
        health_headline.set_xalign(0.5);
        health_headline.set_wrap(true);
        health_headline.set_justify(gtk::Justification::Center);
        card1.append(&health_headline);

        let health_caption = label("\u{2014}", &["caption", "muted"]);
        health_caption.set_halign(gtk::Align::Center);
        health_caption.set_xalign(0.5);
        health_caption.set_wrap(true);
        health_caption.set_justify(gtk::Justification::Center);
        card1.append(&health_caption);

        // Mode switch for bar chart (Memory vs Speed)
        let seg_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        seg_box.add_css_class("seg");
        seg_box.set_halign(gtk::Align::Center);

        let btn_mem = gtk::ToggleButton::with_label("Memory");
        btn_mem.add_css_class("btn");
        btn_mem.update_property(&[gtk::accessible::Property::Label("Show memory per part")]);
        btn_mem.set_active(true);

        let btn_speed = gtk::ToggleButton::with_label("Speed");
        btn_speed.add_css_class("btn");
        btn_speed.update_property(&[gtk::accessible::Property::Label("Show speed per part")]);
        btn_speed.set_group(Some(&btn_mem));

        seg_box.append(&btn_mem);
        seg_box.append(&btn_speed);
        card1.append(&seg_box);

        let bar_chart = BarChart::new(150, "Part performance chart");
        card1.append(bar_chart.widget());

        let bar_caption = label(
            "Memory used, as a share of each part's budget.",
            &["caption", "muted"],
        );
        bar_caption.set_wrap(true);
        bar_caption.set_xalign(0.5);
        bar_caption.set_justify(gtk::Justification::Center);
        card1.append(&bar_caption);

        flow1.append(&card1);

        // -------------------------------------------------------------
        // Card 2: Memory Budget Card
        // -------------------------------------------------------------
        let card2 = card(&["raised"]);
        card2.set_hexpand(true);
        card2.set_size_request(300, -1);

        card2.append(&label("MEMORY BUDGET", &["title-card"]));

        // Side-by-side Spent & Available numbers
        let stats_row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        stats_row.set_homogeneous(true);

        let spent_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
        spent_box.append(&label("SPENT", &["caption", "muted"]));
        let spent_val = label("\u{2014}", &["metric-md", "num"]);
        spent_box.append(&spent_val);
        stats_row.append(&spent_box);

        let avail_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
        avail_box.append(&label("AVAILABLE", &["caption", "muted"]));
        let avail_val = label("\u{2014}", &["metric-md", "num"]);
        avail_box.append(&avail_val);
        stats_row.append(&avail_box);

        card2.append(&stats_row);

        // Slim bar with caption
        let budget_bar = bar();
        card2.append(&budget_bar);

        let budget_caption = label("\u{2014}", &["caption", "muted"]);
        budget_caption.set_wrap(true);
        card2.append(&budget_caption);

        // Donut with total overlay
        let donut = Donut::new(150, "Memory distribution donut chart");
        let donut_overlay = gtk::Overlay::new();
        donut_overlay.set_child(Some(donut.widget()));
        donut_overlay.set_halign(gtk::Align::Center);

        let center_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        center_box.set_halign(gtk::Align::Center);
        center_box.set_valign(gtk::Align::Center);
        center_box.set_can_target(false);
        center_box.append(&label("Total", &["caption", "muted"]));
        let donut_total_val = label("\u{2014}", &["metric-md", "num"]);
        donut_total_val.set_halign(gtk::Align::Center);
        center_box.append(&donut_total_val);

        donut_overlay.add_overlay(&center_box);
        card2.append(&donut_overlay);

        // Legend FlowBox
        let legend_box = gtk::FlowBox::new();
        legend_box.set_selection_mode(gtk::SelectionMode::None);
        legend_box.set_min_children_per_line(1);
        legend_box.set_max_children_per_line(2);
        legend_box.set_homogeneous(false);
        legend_box.set_column_spacing(12);
        legend_box.set_row_spacing(6);
        card2.append(&legend_box);

        flow1.append(&card2);

        // -------------------------------------------------------------
        // Card 3: Voice Card
        // -------------------------------------------------------------
        let voice_card = VoiceCard::new(&ctx.controller);
        let voice_widget = voice_card.widget();
        voice_widget.set_hexpand(true);
        voice_widget.set_size_request(300, -1);
        flow1.append(&voice_widget);

        content.append(&flow1);

        // -------------------------------------------------------------
        // Row 2: Two-column FlowBox (Quick Questions + Last Activity)
        // -------------------------------------------------------------
        let flow2 = gtk::FlowBox::new();
        flow2.set_selection_mode(gtk::SelectionMode::None);
        flow2.set_min_children_per_line(1);
        flow2.set_max_children_per_line(2);
        flow2.set_homogeneous(false);
        flow2.set_column_spacing(20);
        flow2.set_row_spacing(20);

        // Card A: Quick Questions
        let card_q = card(&["raised"]);
        card_q.set_hexpand(true);
        card_q.set_size_request(300, -1);
        card_q.append(&label("QUICK QUESTIONS", &["title-card"]));

        let prompt_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
        for question in [
            "What can you do?",
            "How much battery is left?",
            "What was I working on today?",
        ] {
            let btn = gtk::Button::with_label(question);
            btn.add_css_class("example-prompt");
            btn.set_hexpand(true);
            btn.update_property(&[gtk::accessible::Property::Label(question)]);
            let ask = ctx.ask.clone();
            btn.connect_clicked(move |_| ask(question));
            prompt_box.append(&btn);
        }
        card_q.append(&prompt_box);
        flow2.append(&card_q);

        // Card B: Last Activity
        let card_act = card(&["raised"]);
        card_act.set_hexpand(true);
        card_act.set_size_request(300, -1);
        card_act.append(&label("LAST ACTIVITY", &["title-card"]));

        let audit_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        for _ in 0..5 {
            audit_box.append(&skeleton(160));
        }
        card_act.append(&audit_box);
        flow2.append(&card_act);

        content.append(&flow2);

        // -------------------------------------------------------------
        // Row 3: Parts Grid Section
        // -------------------------------------------------------------
        let parts_section = gtk::Box::new(gtk::Orientation::Vertical, 12);
        parts_section.append(&label("Parts", &["title-section"]));

        let parts_flow = gtk::FlowBox::new();
        parts_flow.set_selection_mode(gtk::SelectionMode::None);
        parts_flow.set_min_children_per_line(1);
        parts_flow.set_max_children_per_line(4);
        parts_flow.set_homogeneous(true);
        parts_flow.set_column_spacing(12);
        parts_flow.set_row_spacing(12);

        let initial_snap = Snapshot::offline();
        let mut part_tiles = Vec::with_capacity(Part::ALL.len());

        for part in Part::ALL {
            let tile_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
            tile_box.add_css_class("tile");
            tile_box.set_hexpand(true);

            let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let state = initial_snap
                .parts
                .iter()
                .find(|p| p.part == part)
                .map_or(PartState::Starting, |p| p.state);
            let dot_widget = dot(state);
            head.append(&dot_widget);

            let title_text = initial_snap
                .parts
                .iter()
                .find(|p| p.part == part)
                .map_or(part_short_label(part), |p| p.title);
            let title_lbl = label(title_text, &["body"]);
            title_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
            head.append(&title_lbl);
            tile_box.append(&head);

            let state_lbl = label("\u{2014}", &["caption"]);
            tile_box.append(&state_lbl);

            let mem_lbl = label("\u{2014}", &["caption", "num", "muted"]);
            tile_box.append(&mem_lbl);

            let gesture = gtk::GestureClick::new();
            let nav = ctx.navigate.clone();
            gesture.connect_pressed(move |_, _, _, _| {
                nav(PageId::Activity);
            });
            tile_box.add_controller(gesture);

            parts_flow.append(&tile_box);
            part_tiles.push((
                part,
                PartTileWidgets {
                    dot: dot_widget,
                    state_label: state_lbl,
                    mem_label: mem_lbl,
                },
            ));
        }

        parts_section.append(&parts_flow);
        content.append(&parts_section);

        root.set_child(Some(&content));

        let chart_mode = Rc::new(Cell::new(BarChartMode::Memory));

        let page = Self {
            root,
            greet_label,
            orb,
            health_headline,
            health_caption,
            bar_chart,
            bar_caption,
            chart_mode: Rc::clone(&chart_mode),
            spent_val,
            avail_val,
            budget_bar,
            budget_caption,
            donut,
            donut_total_val,
            legend_box,
            voice_card,
            audit_box,
            part_tiles,
        };

        // Wire segmented control handlers
        {
            let mode = Rc::clone(&chart_mode);
            let state_ref = ctx.state.clone();
            let chart = page.bar_chart.clone();
            let caption = page.bar_caption.clone();

            btn_mem.connect_toggled(move |btn| {
                if btn.is_active() {
                    mode.set(BarChartMode::Memory);
                    caption.set_text("Memory used, as a share of each part's budget.");
                    let st = state_ref.borrow();
                    Self::update_bars(&chart, &st.snapshot, BarChartMode::Memory);
                }
            });
        }
        {
            let mode = Rc::clone(&chart_mode);
            let state_ref = ctx.state.clone();
            let chart = page.bar_chart.clone();
            let caption = page.bar_caption.clone();

            btn_speed.connect_toggled(move |btn| {
                if btn.is_active() {
                    mode.set(BarChartMode::Speed);
                    caption.set_text(
                        "Slowest 1% of requests, relative to the slowest part. A dash means nothing was measured.",
                    );
                    let st = state_ref.borrow();
                    Self::update_bars(&chart, &st.snapshot, BarChartMode::Speed);
                }
            });
        }

        // Initial paint from state
        let st = ctx.state.borrow();
        page.voice_card.refresh(&st.voice);
        page.redraw_health(&st);
        page.redraw_ledgers(&st);

        page
    }

    fn update_bars(chart: &BarChart, snap: &Snapshot, mode: BarChartMode) {
        let fractions = match mode {
            BarChartMode::Memory => memory_bar_fractions(&snap.parts),
            BarChartMode::Speed => speed_bar_fractions(&snap.parts),
        };
        let mut bars = Vec::with_capacity(Part::ALL.len());
        for (i, part) in Part::ALL.iter().enumerate() {
            bars.push(Bar {
                label: part_short_label(*part).to_owned(),
                fraction: fractions.get(i).copied().unwrap_or(0.0),
                series: i,
            });
        }
        chart.set(&bars);
    }

    fn redraw_health(&self, st: &AppState) {
        let snap = &st.snapshot;
        let hour = glib::DateTime::now_local().map_or(12, |dt| dt.hour());

        // Header greeting line
        self.greet_label
            .set_text(&header_sentence(hour, &st.headline_text()));

        // Card 1: Health
        let headline = snap.headline();
        self.orb.set_tint(orb_tint(&headline, st.have_snapshot));
        self.health_headline.set_text(&st.headline_text());

        let cap_text = if st.have_snapshot {
            format!("{} of {} parts running", snap.healthy, snap.total)
        } else {
            "Connecting\u{2026}".to_owned()
        };
        self.health_caption.set_text(&cap_text);

        // Update bar chart in current mode
        Self::update_bars(&self.bar_chart, snap, self.chart_mode.get());

        // Card 2: Memory Budget
        if st.have_snapshot {
            let total_rss_bytes = snap.total_rss_mib * 1024 * 1024;
            let (spent, avail) = memory_spent_available(total_rss_bytes, snap.total_budget_mib);

            self.spent_val.set_text(&format::bytes_iec(spent));
            self.avail_val.set_text(&format::bytes_iec(avail));

            #[allow(clippy::cast_precision_loss)]
            let fraction = if snap.total_budget_mib > 0 {
                (snap.total_rss_mib as f64) / (snap.total_budget_mib as f64)
            } else {
                0.0
            };
            set_bar(&self.budget_bar, fraction);

            self.budget_caption.set_text(&format!(
                "{} of {} MiB",
                snap.total_rss_mib, snap.total_budget_mib
            ));

            // Donut segments
            let mut donut_segs = Vec::with_capacity(Part::ALL.len());
            for (i, part) in Part::ALL.iter().enumerate() {
                let bytes = snap
                    .parts
                    .iter()
                    .find(|p| p.part == *part)
                    .map_or(0, |p| p.rss_bytes);
                #[allow(clippy::cast_precision_loss)]
                donut_segs.push((bytes as f64, i));
            }
            self.donut.set(&donut_segs);
            self.donut_total_val
                .set_text(&format::bytes_iec(total_rss_bytes));

            // Legend rows
            while let Some(child) = self.legend_box.first_child() {
                self.legend_box.remove(&child);
            }
            for (i, part) in Part::ALL.iter().enumerate() {
                let Some(card_data) = snap.parts.iter().find(|p| p.part == *part) else {
                    continue;
                };
                if card_data.rss_bytes == 0 {
                    continue;
                }
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                row.set_hexpand(true);

                let dot_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                dot_box.add_css_class("legend-dot");
                dot_box.add_css_class(&series_css_class(i));
                dot_box.set_valign(gtk::Align::Center);
                row.append(&dot_box);

                let title_lbl = label(card_data.title, &["caption"]);
                title_lbl.set_hexpand(true);
                title_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
                row.append(&title_lbl);

                let mib = card_data.rss_bytes / (1024 * 1024);
                let val_lbl = label(&format!("{mib} MiB"), &["caption", "num", "muted"]);
                row.append(&val_lbl);

                self.legend_box.append(&row);
            }
        } else {
            self.spent_val.set_text("\u{2014}");
            self.avail_val.set_text("\u{2014}");
            set_bar(&self.budget_bar, 0.0);
            self.budget_caption.set_text("\u{2014}");
            self.donut.set(&[]);
            self.donut_total_val.set_text("\u{2014}");
            while let Some(child) = self.legend_box.first_child() {
                self.legend_box.remove(&child);
            }
            self.legend_box
                .append(&label("\u{2014}", &["caption", "muted"]));
        }

        // Row 3: Parts grid
        for (part, widgets) in &self.part_tiles {
            let Some(card_data) = snap.parts.iter().find(|p| p.part == *part) else {
                continue;
            };
            set_dot(&widgets.dot, card_data.state);

            if st.have_snapshot {
                let word = state_word(card_data.state);
                widgets.state_label.set_text(word);
                for cls in ["state-ok", "state-degraded", "state-down", "state-starting"] {
                    widgets.state_label.remove_css_class(cls);
                }
                widgets
                    .state_label
                    .add_css_class(&format!("state-{}", state_class(card_data.state)));

                let mib = card_data.rss_bytes / (1024 * 1024);
                widgets
                    .mem_label
                    .set_text(&format!("{mib} of {} MiB", card_data.budget_mib));
            } else {
                widgets.state_label.set_text("\u{2014}");
                widgets.mem_label.set_text("\u{2014}");
            }
        }
    }

    fn redraw_ledgers(&self, st: &AppState) {
        while let Some(child) = self.audit_box.first_child() {
            self.audit_box.remove(&child);
        }

        match &st.ledgers.audit {
            None => {
                for _ in 0..5 {
                    self.audit_box.append(&skeleton(160));
                }
            }
            Some(Err(err)) => {
                let err_lbl = label(err, &["caption", "state-down"]);
                err_lbl.set_wrap(true);
                self.audit_box.append(&err_lbl);
            }
            Some(Ok(rows)) if rows.is_empty() => {
                let empty_lbl = label("Nothing yet.", &["caption", "muted"]);
                self.audit_box.append(&empty_lbl);
            }
            Some(Ok(rows)) => {
                for row in rows.iter().take(5) {
                    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                    row_box.add_css_class("row-line");
                    row_box.set_valign(gtk::Align::Center);

                    // Badge icon
                    let badge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                    badge.add_css_class("badge-icon");
                    badge.set_halign(gtk::Align::Center);
                    badge.set_valign(gtk::Align::Center);

                    let is_success_result = row.kind == "RESULT" && row.status.contains("EXECUTED");
                    let icon_name = if is_success_result {
                        "octopit-check-symbolic"
                    } else {
                        "octopit-activity-symbolic"
                    };
                    badge.append(&icon(icon_name, 16));
                    row_box.append(&badge);

                    // Skill name and age
                    let text_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
                    text_box.set_hexpand(true);

                    let skill_lbl = label(&row.skill, &["body"]);
                    skill_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    text_box.append(&skill_lbl);

                    if st.snapshot.generated_at_ns > 0 {
                        let age = format::age_label(st.snapshot.generated_at_ns, row.at_ns);
                        let age_lbl = label(&age, &["caption", "muted", "num"]);
                        text_box.append(&age_lbl);
                    }
                    row_box.append(&text_box);

                    // Risk tier chip
                    let t_class = audit_tier_class(&row.tier);
                    let tier_chip = if t_class.is_empty() {
                        label(&row.tier, &["chip"])
                    } else {
                        label(&row.tier, &["chip", t_class])
                    };
                    row_box.append(&tier_chip);

                    self.audit_box.append(&row_box);
                }
            }
        }
    }
}

impl Page for OverviewPage {
    fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    fn refresh(&self, state: &AppState, changed: Changed) {
        if changed.voice {
            self.voice_card.refresh(&state.voice);
        }
        if changed.health {
            self.redraw_health(state);
        }
        if changed.ledgers {
            self.redraw_ledgers(state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neuroos_octopit_core::health::PartState;

    fn sample_card(part: Part, rss_mb: u64, budget_mb: u64, p99_ns: u64) -> PartCard {
        PartCard {
            part,
            title: part_short_label(part),
            role: "Role",
            state: PartState::Ok,
            reachable: true,
            rss_bytes: rss_mb * 1024 * 1024,
            budget_mib: budget_mb,
            p99_ns,
            uptime_secs: 100,
            subsystems: Vec::new(),
        }
    }

    #[test]
    fn test_greeting_hours() {
        assert_eq!(greeting(5), "Good morning.");
        assert_eq!(greeting(11), "Good morning.");
        assert_eq!(greeting(12), "Good afternoon.");
        assert_eq!(greeting(16), "Good afternoon.");
        assert_eq!(greeting(17), "Good evening.");
        assert_eq!(greeting(23), "Good evening.");
        assert_eq!(greeting(0), "Good evening.");
        assert_eq!(greeting(4), "Good evening.");
    }

    #[test]
    fn test_header_sentence() {
        assert_eq!(
            header_sentence(8, "All systems healthy"),
            "Good morning. All systems healthy."
        );
        assert_eq!(
            header_sentence(14, "All systems healthy."),
            "Good afternoon. All systems healthy."
        );
        assert_eq!(
            header_sentence(20, "Connecting\u{2026}"),
            "Good evening. Connecting\u{2026}."
        );
    }

    #[test]
    fn test_part_short_label() {
        assert_eq!(part_short_label(Part::Healthd), "Health");
        assert_eq!(part_short_label(Part::Storage), "Memory");
        assert_eq!(part_short_label(Part::Inference), "Model");
        assert_eq!(part_short_label(Part::Monitor), "Sensor");
        assert_eq!(part_short_label(Part::Kernel), "Kernel");
        assert_eq!(part_short_label(Part::Knowledge), "Assistant");
        assert_eq!(part_short_label(Part::Voice), "Voice");
        assert_eq!(part_short_label(Part::Fetcher), "Fetcher");
    }

    #[test]
    fn test_memory_bar_fractions() {
        let cards = vec![
            sample_card(Part::Healthd, 50, 100, 0),
            sample_card(Part::Storage, 200, 200, 0),
        ];
        let fracs = memory_bar_fractions(&cards);
        assert_eq!(fracs.len(), 8);
        assert!((fracs[0] - 0.5).abs() < 1e-6); // Healthd (50/100)
        assert!((fracs[1] - 1.0).abs() < 1e-6); // Storage (200/200)
        assert_eq!(fracs[2], 0.0); // Missing part is 0.0
    }

    #[test]
    fn test_speed_bar_fractions() {
        let cards = vec![
            sample_card(Part::Inference, 100, 1000, 10_000_000),
            sample_card(Part::Knowledge, 100, 1000, 5_000_000),
        ];
        let fracs = speed_bar_fractions(&cards);
        assert_eq!(fracs.len(), 8);
        // Inference is index 2 in Part::ALL
        assert!((fracs[2] - 1.0).abs() < 1e-6);
        // Knowledge is index 5 in Part::ALL
        assert!((fracs[5] - 0.5).abs() < 1e-6);
        // Empty parts are 0.0
        assert_eq!(fracs[0], 0.0);
    }

    #[test]
    fn test_speed_bar_fractions_all_zero() {
        let cards = vec![sample_card(Part::Healthd, 10, 100, 0)];
        let fracs = speed_bar_fractions(&cards);
        for f in fracs {
            assert_eq!(f, 0.0);
        }
    }

    #[test]
    fn test_memory_spent_available() {
        let (spent, avail) = memory_spent_available(500 * 1024 * 1024, 1024);
        assert_eq!(spent, 500 * 1024 * 1024);
        assert_eq!(avail, 524 * 1024 * 1024);

        // Saturation when spent > budget
        let (spent, avail) = memory_spent_available(2000 * 1024 * 1024, 1024);
        assert_eq!(spent, 2000 * 1024 * 1024);
        assert_eq!(avail, 0);
    }

    #[test]
    fn test_orb_tint_mapping() {
        assert_eq!(orb_tint(&Headline::Healthy, true), OrbTint::Healthy);
        assert_eq!(orb_tint(&Headline::Unreachable, true), OrbTint::Down);
        assert_eq!(
            orb_tint(
                &Headline::Attention {
                    part: Part::Inference,
                    state: PartState::Down
                },
                true
            ),
            OrbTint::Down
        );
        assert_eq!(
            orb_tint(
                &Headline::Attention {
                    part: Part::Inference,
                    state: PartState::Degraded
                },
                true
            ),
            OrbTint::Attention
        );
        assert_eq!(orb_tint(&Headline::Unreachable, false), OrbTint::Healthy);
    }

    #[test]
    fn test_audit_tier_class() {
        assert_eq!(audit_tier_class("SAFE"), "ok");
        assert_eq!(audit_tier_class("REVIEW"), "warn");
        assert_eq!(audit_tier_class("DANGEROUS"), "down");
        assert_eq!(audit_tier_class("UNKNOWN"), "");
    }
}
