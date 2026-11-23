//! The Backends page: model components and GPU readings (docs/design/octopit-ux.md §4.3).

use gtk::prelude::*;
use gtk4 as gtk;

use neuroos_common::paths::Part;
use neuroos_octopit_core::format;
use neuroos_octopit_core::gpu::{GpuReading, GpuState};
use neuroos_octopit_core::health::{PartCard, PartState};
use neuroos_octopit_core::state::{AppState, Changed};

use super::{Ctx, Page};
use crate::widgets::{
    Ring, Spark, bar, card, dot, label, set_bar, set_dot, skeleton, state_class, state_word,
};

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

struct ModelCardUpdate<'a> {
    card_data: &'a PartCard,
    have_snapshot: bool,
    detail: Option<&'a str>,
    subsystems_text: Option<&'a str>,
}

struct ModelCardWidgets {
    root: gtk::Box,
    dot: gtk::Box,
    state_label: gtk::Label,
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

fn build_model_card(
    title: &str,
    role: &str,
    has_mem_bar: bool,
    has_subsystems: bool,
    has_speed: bool,
) -> (gtk::Box, ModelCardWidgets) {
    let root = card(&["backend-card"]);
    root.set_hexpand(true);

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let d = dot(PartState::Starting);
    head.append(&d);
    head.append(&label(title, &["title-section"]));
    root.append(&head);

    let role_lbl = label(role, &["caption"]);
    role_lbl.set_wrap(true);
    root.append(&role_lbl);

    let state_lbl = label("Starting", &["caption"]);
    root.append(&state_lbl);

    let (mem_bar, mem_label) = if has_mem_bar {
        let b = bar();
        root.append(&b);
        let m = label("", &["caption", "num"]);
        root.append(&m);
        (Some(b), m)
    } else {
        let m = label("Shares memory with speech to text", &["caption", "muted"]);
        root.append(&m);
        (None, m)
    };

    let subsystems_label = if has_subsystems {
        let s = label("", &["caption"]);
        s.set_wrap(true);
        root.append(&s);
        Some(s)
    } else {
        None
    };

    let detail_label = label("", &["caption"]);
    detail_label.set_wrap(true);
    root.append(&detail_label);

    let speed_label = if has_speed {
        let sp = label("", &["caption", "num"]);
        root.append(&sp);
        Some(sp)
    } else {
        None
    };

    let widgets = ModelCardWidgets {
        root: root.clone(),
        dot: d,
        state_label: state_lbl,
        mem_bar,
        mem_label,
        detail_label,
        subsystems_label,
        speed_label,
    };

    (root, widgets)
}

fn build_gauge(title: &str, ring: &Ring, value_label: &gtk::Label) -> gtk::Box {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 4);
    col.add_css_class("gpu-gauge-col");
    col.set_hexpand(true);
    col.set_halign(gtk::Align::Center);

    let title_lbl = label(title, &["title-card"]);
    title_lbl.set_halign(gtk::Align::Center);
    title_lbl.set_xalign(0.5);
    col.append(&title_lbl);

    col.append(ring.widget());

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
        let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
        content.add_css_class("page");

        // Header
        content.append(&label("Backends", &["title-page"]));
        content.append(&label(
            "The models that run on this machine. Nothing here leaves it.",
            &["caption"],
        ));

        // Models Grid
        let flow = gtk::FlowBox::new();
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_min_children_per_line(1);
        flow.set_max_children_per_line(2);
        flow.set_column_spacing(12);
        flow.set_row_spacing(12);
        flow.set_homogeneous(true);
        flow.set_activate_on_single_click(false);

        // 1. Language model (Part::Inference)
        let (card0, w0) = build_model_card(
            "Language model",
            "Writes the answers, fully offline.",
            true,
            false,
            true,
        );
        flow.append(&card0);

        // 2. Speech to text (Part::Voice)
        let (card1, w1) = build_model_card(
            "Speech to text",
            "Transcribes speech from the microphone.",
            true,
            false,
            false,
        );
        flow.append(&card1);

        // 3. Voice output (Part::Voice)
        let (card2, w2) = build_model_card("Voice", "Speaks answers aloud.", false, true, false);
        flow.append(&card2);

        // 4. Embeddings (Part::Storage)
        let (card3, w3) = build_model_card(
            "Embeddings",
            "Semantic search for notes and history.",
            true,
            false,
            false,
        );
        flow.append(&card3);

        content.append(&flow);

        // GPU Card
        let gpu_card = card(&["raised"]);
        gpu_card.append(&label("GPU", &["title-section"]));

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
            "NeuroOS never requires a GPU.",
            &["caption", "muted"],
        ));
        gpu_unavailable.set_visible(false);
        gpu_card.append(&gpu_unavailable);

        // GPU present content
        let gpu_present = gtk::Box::new(gtk::Orientation::Vertical, 12);
        gpu_present.set_visible(false);

        let gpu_head = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let gpu_name = label("", &["title-section"]);
        let gpu_driver = label("", &["caption"]);
        gpu_head.append(&gpu_name);
        gpu_head.append(&gpu_driver);
        gpu_present.append(&gpu_head);

        // 3 Gauges
        let ring_busy = Ring::new(96, "GPU utilization");
        let val_busy = label("\u{2014}", &[]);
        let ring_mem = Ring::new(96, "Video memory in use");
        let val_mem = label("\u{2014}", &[]);
        let ring_temp = Ring::new(96, "GPU temperature");
        let val_temp = label("\u{2014}", &[]);

        // A flow box: three across when there is room, fewer when there is not.
        let gauges_row = gtk::FlowBox::new();
        gauges_row.add_css_class("gpu-gauges");
        gauges_row.set_selection_mode(gtk::SelectionMode::None);
        gauges_row.set_min_children_per_line(1);
        gauges_row.set_max_children_per_line(3);
        gauges_row.set_homogeneous(true);
        gauges_row.set_can_focus(false);
        for col in [
            build_gauge("Busy", &ring_busy, &val_busy),
            build_gauge("Video memory", &ring_mem, &val_mem),
            build_gauge("Temperature", &ring_temp, &val_temp),
        ] {
            col.set_size_request(150, -1);
            gauges_row.append(&col);
        }
        gpu_present.append(&gauges_row);

        // Sparklines
        let spark_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
        spark_box.add_css_class("gpu-spark-row");

        let spark_busy = Spark::new(56, "GPU utilization history");
        let spark_busy_caption = label("", &["caption"]);
        spark_box.append(spark_busy.widget());
        spark_box.append(&spark_busy_caption);

        let spark_temp = Spark::new(56, "GPU temperature history");
        let spark_temp_caption = label("", &["caption"]);
        spark_box.append(spark_temp.widget());
        spark_box.append(&spark_temp_caption);
        gpu_present.append(&spark_box);

        // Power row
        let power_label = label("", &["caption"]);
        gpu_present.append(&power_label);

        // Vendor caption
        let vendor_cap = label(
            "Read with the GPU vendor's own tool, once every two seconds.",
            &["caption", "muted"],
        );
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

        set_dot(&w.dot, card_data.state);

        w.root.remove_css_class("down");
        w.root.remove_css_class("attention");

        if !card_data.reachable || card_data.state == PartState::Down {
            w.root.add_css_class("down");
            w.state_label.set_text("Not running");
        } else if card_data.state == PartState::Degraded {
            w.root.add_css_class("attention");
            w.state_label.set_text(state_word(card_data.state));
        } else if update.have_snapshot {
            w.state_label.set_text(state_word(card_data.state));
        } else {
            w.state_label.set_text("\u{2014}");
        }

        for c in ["state-ok", "state-degraded", "state-down", "state-starting"] {
            w.state_label.remove_css_class(c);
        }
        w.state_label
            .add_css_class(&format!("state-{}", state_class(card_data.state)));

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
}
