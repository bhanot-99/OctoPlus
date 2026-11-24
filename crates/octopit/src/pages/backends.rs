//! The Backends page: model components and GPU readings (docs/design/octopit-ux.md §4.3).

use gtk::prelude::*;
use gtk4 as gtk;

use octoplus_common::paths::Part;
use octopit_core::format;
use octopit_core::gpu::{GpuReading, GpuState};
use octopit_core::health::{PartCard, PartState};
use octopit_core::state::{AppState, Changed};

use super::{Ctx, Page};
use crate::widgets::{Ring, Spark, bar, card, icon, label, set_bar, skeleton};

/// Returns the CSS class ("warn", "hot", or "") for a gauge fraction `0.0..=1.0`.
#[must_use]
pub fn gauge_class(fraction: f64) -> &'static str {
    if fraction >= 0.95 {
        "hot"
    } else if fraction >= 0.8 {
        "warn"
    } else {
        ""
    }
}

/// Returns the CSS class ("warn", "hot", or "") for temperature in Celsius.
#[must_use]
pub fn temperature_class(temp_c: f64) -> &'static str {
    if temp_c >= 90.0 {
        "hot"
    } else if temp_c >= 80.0 {
        "warn"
    } else {
        ""
    }
}

/// Scales temperature in Celsius to a ring fraction `0.0..=1.0` (0 to 100 °C clamped).
#[must_use]
pub fn temperature_fraction(temp_c: f64) -> f64 {
    (temp_c / 100.0).clamp(0.0, 1.0)
}

/// Formats memory usage and budget as "N of M MiB".
#[must_use]
pub fn format_memory_mib(rss_bytes: u64, budget_mib: u64) -> String {
    let rss_mib = rss_bytes / (1024 * 1024);
    format!("{rss_mib} of {budget_mib} MiB")
}

/// Formats GPU memory as "N MiB of M MiB" or dash when unavailable.
#[must_use]
pub fn format_gpu_memory(used: Option<f64>, total: Option<f64>) -> String {
    match (used, total) {
        (Some(u), Some(t)) => format!("{u:.0} MiB of {t:.0} MiB"),
        _ => "\u{2014}".to_owned(),
    }
}

/// Truncates a detail string to at most 60 characters.
#[must_use]
pub fn cap_detail(detail: &str) -> String {
    detail.chars().take(60).collect()
}

/// Formats a sparkline caption line giving the latest value and the min/max range.
#[must_use]
pub fn format_spark_caption(
    title: &str,
    latest: Option<f64>,
    range: Option<(f64, f64)>,
    unit: &str,
) -> String {
    match (latest, range) {
        (Some(cur), Some((min, max))) => {
            format!("{title}: {cur:.0}{unit} (min {min:.0}{unit}, max {max:.0}{unit})")
        }
        _ => format!("{title}: \u{2014}"),
    }
}

/// Formats the power draw and optional power limit.
#[must_use]
pub fn format_power_row(power_w: Option<f64>, limit_w: Option<f64>) -> Option<String> {
    let p = power_w?;
    match limit_w {
        Some(lim) if lim > 0.0 => Some(format!("Power draw {p:.0} W of {lim:.0} W limit")),
        _ => Some(format!("Power draw {p:.0} W")),
    }
}

/// Returns the chip text and CSS class modifier for a backend component state.
#[must_use]
pub fn model_state_chip(
    state: PartState,
    reachable: bool,
    have_snapshot: bool,
) -> (&'static str, &'static str) {
    if !have_snapshot {
        ("\u{2014}", "")
    } else if !reachable || state == PartState::Down {
        ("Down", "down")
    } else if state == PartState::Degraded {
        ("Degraded", "warn")
    } else if state == PartState::Starting {
        ("Starting", "warn")
    } else {
        ("Running", "ok")
    }
}

struct ModelCardUpdate<'a> {
    card_data: &'a PartCard,
    have_snapshot: bool,
    detail: Option<&'a str>,
    subsystems_text: Option<&'a str>,
}

struct ModelCardWidgets {
    root: gtk::Box,
    state_chip: gtk::Label,
    mem_bar: Option<gtk::ProgressBar>,
    mem_label: gtk::Label,
    detail_label: gtk::Label,
    subsystems_label: Option<gtk::Label>,
    speed_label: Option<gtk::Label>,
}

/// The Backends page.
pub struct BackendsPage {
    root: gtk::ScrolledWindow,
    // 4 model cards:
    // 0: Language model (Inference)
    // 1: Speech to text (Voice - stt)
    // 2: Voice output (Voice - tts/wake)
    // 3: Embeddings (Storage - embedder)
    model_cards: [ModelCardWidgets; 4],

    // GPU card
    gpu_loading: gtk::Box,
    gpu_unavailable: gtk::Box,
    gpu_present: gtk::Box,

    // GPU present widgets
    gpu_name: gtk::Label,
    gpu_driver: gtk::Label,
    ring_busy: Ring,
    val_busy: gtk::Label,
    ring_mem: Ring,
    val_mem: gtk::Label,
    ring_temp: Ring,
    val_temp: gtk::Label,
    spark_busy: Spark,
    spark_busy_caption: gtk::Label,
    spark_temp: Spark,
    spark_temp_caption: gtk::Label,
    power_label: gtk::Label,
}

struct ModelCardSpec<'a> {
    title: &'a str,
    role: &'a str,
    icon_name: &'a str,
    has_mem_bar: bool,
    has_subsystems: bool,
    has_speed: bool,
}

fn build_model_card(spec: &ModelCardSpec<'_>) -> (gtk::Box, ModelCardWidgets) {
    let root = card(&["backend-card"]);
    root.set_hexpand(true);
    root.set_size_request(300, -1);

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    head.set_valign(gtk::Align::Center);

    let badge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    badge.add_css_class("badge-icon");
    badge.set_halign(gtk::Align::Center);
    badge.set_valign(gtk::Align::Center);
    badge.append(&icon(spec.icon_name, 16));
    head.append(&badge);

    let title_lbl = label(spec.title, &["title-section"]);
    title_lbl.set_hexpand(true);
    title_lbl.set_xalign(0.0);
    title_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    head.append(&title_lbl);

    let state_chip = label("Starting", &["chip"]);
    state_chip.set_valign(gtk::Align::Center);
    head.append(&state_chip);

    root.append(&head);

    let role_lbl = label(spec.role, &["caption"]);
    role_lbl.set_wrap(true);
    role_lbl.set_xalign(0.0);
    root.append(&role_lbl);

    let (mem_bar, mem_label) = if spec.has_mem_bar {
        let b = bar();
        root.append(&b);
        let m = label("", &["caption", "num"]);
        m.set_xalign(0.0);
        root.append(&m);
        (Some(b), m)
    } else {
        let m = label("Shares memory with speech to text", &["caption", "muted"]);
        m.set_xalign(0.0);
        root.append(&m);
        (None, m)
    };

    let subsystems_label = if spec.has_subsystems {
        let s = label("", &["caption"]);
        s.set_wrap(true);
        s.set_xalign(0.0);
        root.append(&s);
        Some(s)
    } else {
        None
    };

    let detail_label = label("", &["caption"]);
    detail_label.set_wrap(true);
    detail_label.set_xalign(0.0);
    root.append(&detail_label);

    let speed_label = if spec.has_speed {
        let sp = label("", &["caption", "num"]);
        sp.set_xalign(0.0);
        root.append(&sp);
        Some(sp)
    } else {
        None
    };

    let widgets = ModelCardWidgets {
        root: root.clone(),
        state_chip,
        mem_bar,
        mem_label,
        detail_label,
        subsystems_label,
        speed_label,
    };

    (root, widgets)
}

fn build_gauge(title: &str, ring: &Ring, value_label: &gtk::Label) -> gtk::Box {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 8);
    col.add_css_class("tile");
    col.add_css_class("gpu-gauge-col");
    col.set_hexpand(true);

    let title_lbl = label(title, &["title-card"]);
    title_lbl.set_halign(gtk::Align::Center);
    title_lbl.set_xalign(0.5);
    col.append(&title_lbl);

    let ring_widget = ring.widget();
    ring_widget.set_halign(gtk::Align::Center);
    col.append(ring_widget);

    value_label.add_css_class("metric-md");
    value_label.add_css_class("num");
    value_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    value_label.set_max_width_chars(18);
    value_label.set_halign(gtk::Align::Center);
    value_label.set_xalign(0.5);
    col.append(value_label);

    col
}

impl BackendsPage {
    /// Builds the page.
    #[must_use]
    pub fn new(_ctx: &Ctx) -> Self {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 20);
        content.add_css_class("page");

        // Header
        let title_lbl = label("Backends", &["title-page"]);
        let greet_lbl = label(
            "The models that run on this machine. Nothing here leaves it.",
            &["title-greet"],
        );
        greet_lbl.set_wrap(true);
        content.append(&title_lbl);
        content.append(&greet_lbl);

        // Models Grid
        let flow = gtk::FlowBox::new();
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_min_children_per_line(1);
        flow.set_max_children_per_line(2);
        flow.set_column_spacing(16);
        flow.set_row_spacing(16);
        flow.set_homogeneous(true);
        flow.set_activate_on_single_click(false);

        // 1. Language model (Part::Inference)
        let (card0, w0) = build_model_card(&ModelCardSpec {
            title: "Language model",
            role: "Writes the answers, fully offline.",
            icon_name: "octopit-chat-symbolic",
            has_mem_bar: true,
            has_subsystems: false,
            has_speed: true,
        });
        flow.append(&card0);

        // 2. Speech to text (Part::Voice)
        let (card1, w1) = build_model_card(&ModelCardSpec {
            title: "Speech to text",
            role: "Transcribes speech from the microphone.",
            icon_name: "octopit-mic-symbolic",
            has_mem_bar: true,
            has_subsystems: false,
            has_speed: false,
        });
        flow.append(&card1);

        // 3. Voice output (Part::Voice)
        let (card2, w2) = build_model_card(&ModelCardSpec {
            title: "Voice",
            role: "Speaks answers aloud.",
            icon_name: "octopit-mic-symbolic",
            has_mem_bar: false,
            has_subsystems: true,
            has_speed: false,
        });
        flow.append(&card2);

        // 4. Embeddings (Part::Storage)
        let (card3, w3) = build_model_card(&ModelCardSpec {
            title: "Embeddings",
            role: "Semantic search for notes and history.",
            icon_name: "octopit-backends-symbolic",
            has_mem_bar: true,
            has_subsystems: false,
            has_speed: false,
        });
        flow.append(&card3);

        content.append(&flow);

        // GPU Card
        let gpu_card = card(&[]);
        gpu_card.append(&label("GPU", &["title-card"]));

        // GPU loading skeleton
        let gpu_loading = gtk::Box::new(gtk::Orientation::Vertical, 8);
        gpu_loading.append(&skeleton(280));
        gpu_loading.append(&skeleton(160));
        gpu_card.append(&gpu_loading);

        // GPU unavailable calm message
        let gpu_unavailable = gtk::Box::new(gtk::Orientation::Vertical, 4);
        gpu_unavailable.append(&label(
            "No supported GPU found. Everything runs on the CPU, which is fine.",
            &["body"],
        ));
        gpu_unavailable.append(&label(
            "OctoPlus never requires a GPU.",
            &["caption", "muted"],
        ));
        gpu_unavailable.set_visible(false);
        gpu_card.append(&gpu_unavailable);

        // GPU present content
        let gpu_present = gtk::Box::new(gtk::Orientation::Vertical, 16);
        gpu_present.set_visible(false);

        // Header row with badge-icon, GPU name, and Driver caption
        let gpu_head_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        gpu_head_row.set_valign(gtk::Align::Center);

        let badge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        badge.add_css_class("badge-icon");
        badge.set_halign(gtk::Align::Center);
        badge.set_valign(gtk::Align::Center);
        badge.append(&icon("octopit-backends-symbolic", 16));
        gpu_head_row.append(&badge);

        let gpu_title_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
        gpu_title_box.set_hexpand(true);
        let gpu_name = label("", &["title-section"]);
        gpu_name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        gpu_name.set_xalign(0.0);
        let gpu_driver = label("", &["caption", "muted"]);
        gpu_driver.set_xalign(0.0);
        gpu_title_box.append(&gpu_name);
        gpu_title_box.append(&gpu_driver);
        gpu_head_row.append(&gpu_title_box);
        gpu_present.append(&gpu_head_row);

        // 3 Gauges inside tile boxes in a FlowBox
        let ring_busy = Ring::new(96, "GPU utilization");
        let val_busy = label("\u{2014}", &[]);
        let ring_mem = Ring::new(96, "Video memory in use");
        let val_mem = label("\u{2014}", &[]);
        let ring_temp = Ring::new(96, "GPU temperature");
        let val_temp = label("\u{2014}", &[]);

        let gauges_row = gtk::FlowBox::new();
        gauges_row.add_css_class("gpu-gauges");
        gauges_row.set_selection_mode(gtk::SelectionMode::None);
        gauges_row.set_min_children_per_line(1);
        gauges_row.set_max_children_per_line(3);
        gauges_row.set_column_spacing(12);
        gauges_row.set_row_spacing(12);
        gauges_row.set_homogeneous(true);
        gauges_row.set_can_focus(false);

        for col in [
            build_gauge("BUSY", &ring_busy, &val_busy),
            build_gauge("VIDEO MEMORY", &ring_mem, &val_mem),
            build_gauge("TEMPERATURE", &ring_temp, &val_temp),
        ] {
            col.set_size_request(150, -1);
            gauges_row.append(&col);
        }
        gpu_present.append(&gauges_row);

        // Sparklines each in their own tile with its caption
        let spark_flow = gtk::FlowBox::new();
        spark_flow.set_selection_mode(gtk::SelectionMode::None);
        spark_flow.set_min_children_per_line(1);
        spark_flow.set_max_children_per_line(2);
        spark_flow.set_column_spacing(12);
        spark_flow.set_row_spacing(12);
        spark_flow.set_homogeneous(true);
        spark_flow.set_can_focus(false);

        let tile_busy = gtk::Box::new(gtk::Orientation::Vertical, 8);
        tile_busy.add_css_class("tile");
        tile_busy.set_hexpand(true);
        let spark_busy = Spark::new(56, "GPU utilization history");
        let spark_busy_caption = label("", &["caption"]);
        tile_busy.append(spark_busy.widget());
        tile_busy.append(&spark_busy_caption);
        spark_flow.append(&tile_busy);

        let tile_temp = gtk::Box::new(gtk::Orientation::Vertical, 8);
        tile_temp.add_css_class("tile");
        tile_temp.set_hexpand(true);
        let spark_temp = Spark::new(56, "GPU temperature history");
        let spark_temp_caption = label("", &["caption"]);
        tile_temp.append(spark_temp.widget());
        tile_temp.append(&spark_temp_caption);
        spark_flow.append(&tile_temp);

        gpu_present.append(&spark_flow);

        // Power row
        let power_label = label("", &["caption"]);
        power_label.set_xalign(0.0);
        gpu_present.append(&power_label);

        // Vendor caption
        let vendor_cap = label(
            "Read with the GPU vendor's own tool, once every two seconds.",
            &["caption", "muted"],
        );
        vendor_cap.set_xalign(0.0);
        vendor_cap.set_wrap(true);
        gpu_present.append(&vendor_cap);

        gpu_card.append(&gpu_present);
        content.append(&gpu_card);

        let root = gtk::ScrolledWindow::new();
        root.set_child(Some(&content));
        root.set_hscrollbar_policy(gtk::PolicyType::Automatic);

        Self {
            root,
            model_cards: [w0, w1, w2, w3],
            gpu_loading,
            gpu_unavailable,
            gpu_present,
            gpu_name,
            gpu_driver,
            ring_busy,
            val_busy,
            ring_mem,
            val_mem,
            ring_temp,
            val_temp,
            spark_busy,
            spark_busy_caption,
            spark_temp,
            spark_temp_caption,
            power_label,
        }
    }

    fn redraw_models(&self, state: &AppState) {
        let snap = &state.snapshot;

        // Card 0: Language model (Inference)
        if let Some(card) = snap.parts.iter().find(|p| p.part == Part::Inference) {
            let sub = card.subsystems.iter().find(|s| s.name == "model");
            let detail = sub.map_or("", |s| s.detail.as_str());
            self.update_card(
                0,
                &ModelCardUpdate {
                    card_data: card,
                    have_snapshot: state.have_snapshot,
                    detail: Some(detail),
                    subsystems_text: None,
                },
            );
        }

        // Card 1: Speech to text (Voice - stt)
        if let Some(card) = snap.parts.iter().find(|p| p.part == Part::Voice) {
            let sub = card.subsystems.iter().find(|s| s.name == "stt");
            let detail = sub.map_or("", |s| s.detail.as_str());
            self.update_card(
                1,
                &ModelCardUpdate {
                    card_data: card,
                    have_snapshot: state.have_snapshot,
                    detail: Some(detail),
                    subsystems_text: None,
                },
            );
        }

        // Card 2: Voice output (Voice - tts / wake)
        if let Some(card) = snap.parts.iter().find(|p| p.part == Part::Voice) {
            let tts_st = card
                .subsystems
                .iter()
                .find(|s| s.name == "tts")
                .map_or(card.state, |s| s.state);
            let wake_st = card
                .subsystems
                .iter()
                .find(|s| s.name == "wake")
                .map_or(card.state, |s| s.state);
            let subs_text = format!(
                "Speech: {} \u{00B7} Wake word: {}",
                match tts_st {
                    PartState::Ok => "ok",
                    PartState::Degraded => "degraded",
                    PartState::Down => "down",
                    PartState::Starting => "starting",
                },
                match wake_st {
                    PartState::Ok => "ok",
                    PartState::Degraded => "degraded",
                    PartState::Down => "down",
                    PartState::Starting => "starting",
                }
            );
            self.update_card(
                2,
                &ModelCardUpdate {
                    card_data: card,
                    have_snapshot: state.have_snapshot,
                    detail: None,
                    subsystems_text: Some(&subs_text),
                },
            );
        }

        // Card 3: Embeddings (Storage - embedder)
        if let Some(card) = snap.parts.iter().find(|p| p.part == Part::Storage) {
            let sub = card.subsystems.iter().find(|s| s.name == "embedder");
            let detail = sub.map_or("", |s| s.detail.as_str());
            self.update_card(
                3,
                &ModelCardUpdate {
                    card_data: card,
                    have_snapshot: state.have_snapshot,
                    detail: Some(detail),
                    subsystems_text: None,
                },
            );
        }
    }

    fn update_card(&self, index: usize, update: &ModelCardUpdate<'_>) {
        let w = &self.model_cards[index];
        let card_data = update.card_data;

        w.root.remove_css_class("down");
        w.root.remove_css_class("attention");

        if !card_data.reachable || card_data.state == PartState::Down {
            w.root.add_css_class("down");
        } else if card_data.state == PartState::Degraded {
            w.root.add_css_class("attention");
        }

        let (chip_text, chip_cls) =
            model_state_chip(card_data.state, card_data.reachable, update.have_snapshot);
        w.state_chip.set_text(chip_text);
        for c in ["ok", "warn", "down"] {
            w.state_chip.remove_css_class(c);
        }
        if !chip_cls.is_empty() {
            w.state_chip.add_css_class(chip_cls);
        }

        // Memory bar
        if let Some(ref b) = w.mem_bar {
            set_bar(b, card_data.rss_fraction());
            w.mem_label.set_text(&format_memory_mib(
                card_data.rss_bytes,
                card_data.budget_mib,
            ));
        }

        // Subsystems line
        if let (Some(s_lbl), Some(st_text)) = (&w.subsystems_label, update.subsystems_text) {
            s_lbl.set_text(st_text);
        }

        // Detail line
        if let Some(d) = update.detail {
            if d.is_empty() {
                w.detail_label.set_visible(false);
            } else {
                w.detail_label.set_text(&cap_detail(d));
                w.detail_label.set_visible(true);
            }
        }

        // Speed line
        if let Some(ref sp) = w.speed_label {
            if card_data.p99_ns > 0 {
                sp.set_text(&format!(
                    "Slowest 1% per token: {}",
                    format::latency(card_data.p99_ns)
                ));
                sp.set_visible(true);
            } else {
                sp.set_visible(false);
            }
        }
    }

    fn redraw_gpu(&self, state: &AppState) {
        match &state.gpu {
            None => {
                self.gpu_loading.set_visible(true);
                self.gpu_unavailable.set_visible(false);
                self.gpu_present.set_visible(false);
            }
            Some(GpuState::NotAvailable) => {
                self.gpu_loading.set_visible(false);
                self.gpu_unavailable.set_visible(true);
                self.gpu_present.set_visible(false);
            }
            Some(GpuState::Present(r)) => {
                self.gpu_loading.set_visible(false);
                self.gpu_unavailable.set_visible(false);
                self.gpu_present.set_visible(true);
                self.update_gpu_present(r, state);
            }
        }
    }

    fn update_gpu_present(&self, r: &GpuReading, state: &AppState) {
        self.gpu_name.set_text(&r.name);
        self.gpu_driver.set_text(&format!("Driver {}", r.driver));

        // 1. Busy Gauge
        let (busy_frac, busy_text) = match r.utilization_pct {
            Some(u) => ((u / 100.0).clamp(0.0, 1.0), format::percent(u / 100.0)),
            None => (0.0, "\u{2014}".to_owned()),
        };
        self.ring_busy.set(busy_frac);
        self.val_busy.set_text(&busy_text);
        let ring_w = self.ring_busy.widget();
        ring_w.remove_css_class("warn");
        ring_w.remove_css_class("hot");
        let cls = gauge_class(busy_frac);
        if !cls.is_empty() {
            ring_w.add_css_class(cls);
        }

        // 2. Video Memory Gauge
        let (mem_frac, mem_text) = match r.memory_fraction() {
            Some(f) => (f, format_gpu_memory(r.memory_used_mib, r.memory_total_mib)),
            None => (0.0, "\u{2014}".to_owned()),
        };
        self.ring_mem.set(mem_frac);
        self.val_mem.set_text(&mem_text);
        let ring_w = self.ring_mem.widget();
        ring_w.remove_css_class("warn");
        ring_w.remove_css_class("hot");
        let cls = gauge_class(mem_frac);
        if !cls.is_empty() {
            ring_w.add_css_class(cls);
        }

        // 3. Temperature Gauge
        let (temp_frac, temp_text, temp_c) = match r.temperature_c {
            Some(t) => (temperature_fraction(t), format!("{t:.0} \u{00B0}C"), t),
            None => (0.0, "\u{2014}".to_owned(), 0.0),
        };
        self.ring_temp.set(temp_frac);
        self.val_temp.set_text(&temp_text);
        let ring_w = self.ring_temp.widget();
        ring_w.remove_css_class("warn");
        ring_w.remove_css_class("hot");
        if r.temperature_c.is_some() {
            let cls = temperature_class(temp_c);
            if !cls.is_empty() {
                ring_w.add_css_class(cls);
            }
        }

        // Sparklines
        self.spark_busy.set(state.gpu_util.normalized(0.0, 100.0));
        self.spark_busy_caption.set_text(&format_spark_caption(
            "Busy, last 5 minutes",
            state.gpu_util.last(),
            state.gpu_util.range(),
            "%",
        ));

        self.spark_temp.set(state.gpu_temp.normalized(20.0, 100.0));
        self.spark_temp_caption.set_text(&format_spark_caption(
            "Temperature, last 5 minutes",
            state.gpu_temp.last(),
            state.gpu_temp.range(),
            "\u{00B0}C",
        ));

        // Power row
        if let Some(p_text) = format_power_row(r.power_w, r.power_limit_w) {
            self.power_label.set_text(&p_text);
            self.power_label.set_visible(true);
        } else {
            self.power_label.set_visible(false);
        }
    }
}

impl Page for BackendsPage {
    fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    fn refresh(&self, state: &AppState, changed: Changed) {
        if changed.health {
            self.redraw_models(state);
        }
        if changed.gpu {
            self.redraw_gpu(state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gauge_class() {
        assert_eq!(gauge_class(0.0), "");
        assert_eq!(gauge_class(0.5), "");
        assert_eq!(gauge_class(0.799), "");
        assert_eq!(gauge_class(0.8), "warn");
        assert_eq!(gauge_class(0.9), "warn");
        assert_eq!(gauge_class(0.949), "warn");
        assert_eq!(gauge_class(0.95), "hot");
        assert_eq!(gauge_class(1.0), "hot");
    }

    #[test]
    fn test_temperature_class() {
        assert_eq!(temperature_class(45.0), "");
        assert_eq!(temperature_class(79.9), "");
        assert_eq!(temperature_class(80.0), "warn");
        assert_eq!(temperature_class(89.9), "warn");
        assert_eq!(temperature_class(90.0), "hot");
        assert_eq!(temperature_class(95.0), "hot");
    }

    #[test]
    fn test_temperature_fraction() {
        assert_eq!(temperature_fraction(0.0), 0.0);
        assert_eq!(temperature_fraction(50.0), 0.5);
        assert_eq!(temperature_fraction(100.0), 1.0);
        assert_eq!(temperature_fraction(120.0), 1.0);
        assert_eq!(temperature_fraction(-10.0), 0.0);
    }

    #[test]
    fn test_format_memory_mib() {
        assert_eq!(format_memory_mib(0, 100), "0 of 100 MiB");
        assert_eq!(format_memory_mib(100 * 1024 * 1024, 200), "100 of 200 MiB");
    }

    #[test]
    fn test_format_gpu_memory() {
        assert_eq!(
            format_gpu_memory(Some(1024.0), Some(4096.0)),
            "1024 MiB of 4096 MiB"
        );
        assert_eq!(format_gpu_memory(None, Some(4096.0)), "\u{2014}");
        assert_eq!(format_gpu_memory(Some(1024.0), None), "\u{2014}");
        assert_eq!(format_gpu_memory(None, None), "\u{2014}");
    }

    #[test]
    fn test_cap_detail() {
        assert_eq!(cap_detail("bge-small-en-v1.5"), "bge-small-en-v1.5");
        let long = "a".repeat(100);
        let capped = cap_detail(&long);
        assert_eq!(capped.len(), 60);
    }

    #[test]
    fn test_format_spark_caption() {
        let cap = format_spark_caption("Busy", Some(45.0), Some((10.0, 80.0)), "%");
        assert!(cap.contains("45"));
        assert!(cap.contains("10"));
        assert!(cap.contains("80"));

        let empty = format_spark_caption("Busy", None, None, "%");
        assert_eq!(empty, "Busy: \u{2014}");
    }

    #[test]
    fn test_format_power_row() {
        assert_eq!(
            format_power_row(Some(50.0), Some(100.0)),
            Some("Power draw 50 W of 100 W limit".to_string())
        );
        assert_eq!(
            format_power_row(Some(50.0), None),
            Some("Power draw 50 W".to_string())
        );
        assert_eq!(
            format_power_row(Some(50.0), Some(0.0)),
            Some("Power draw 50 W".to_string())
        );
        assert_eq!(format_power_row(None, Some(100.0)), None);
    }

    #[test]
    fn test_model_state_chip() {
        assert_eq!(
            model_state_chip(PartState::Ok, true, false),
            ("\u{2014}", "")
        );
        assert_eq!(
            model_state_chip(PartState::Ok, true, true),
            ("Running", "ok")
        );
        assert_eq!(
            model_state_chip(PartState::Ok, false, true),
            ("Down", "down")
        );
        assert_eq!(
            model_state_chip(PartState::Down, true, true),
            ("Down", "down")
        );
        assert_eq!(
            model_state_chip(PartState::Degraded, true, true),
            ("Degraded", "warn")
        );
        assert_eq!(
            model_state_chip(PartState::Starting, true, true),
            ("Starting", "warn")
        );
    }
}
