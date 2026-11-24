//! The Activity page: speed, load and audit trail (docs/design/octopit-ux.md §4.4).

use gtk::prelude::*;
use gtk4 as gtk;

use octopit_core::format;
use octopit_core::gateway::{AuditRow, ChainStatus};
use octopit_core::state::{AppState, Changed};
use octoplus_common::paths::Part;

use super::{Ctx, Page};
use crate::widgets::{Spark, bar, card, icon, label, set_bar, skeleton};

/// Returns the symbolic icon name for an audit row.
#[must_use]
pub fn audit_row_icon(kind: &str, status: &str) -> &'static str {
    if kind == "RESULT" && status.contains("EXECUTED") {
        "octopit-check-symbolic"
    } else {
        "octopit-activity-symbolic"
    }
}

/// Maps an audit record tier to its corresponding chip CSS modifier class.
#[must_use]
pub fn tier_class(tier: &str) -> &'static str {
    match tier {
        "SAFE" => "ok",
        "REVIEW" => "warn",
        "DANGEROUS" => "down",
        _ => "",
    }
}

/// Scales a p99 latency value to a fraction `0.0..=1.0` against the maximum p99.
#[must_use]
pub fn scale_p99(p99_ns: u64, max_p99_ns: u64) -> f64 {
    if max_p99_ns == 0 {
        0.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        let fraction = (p99_ns as f64) / (max_p99_ns as f64);
        fraction.clamp(0.0, 1.0)
    }
}

/// Formats the caption for the 5-minute memory sparkline.
#[must_use]
pub fn format_memory_spark_caption(latest_mib: Option<f64>, budget_mib: u64) -> String {
    match latest_mib {
        Some(v) => format!("Latest: {v:.0} MiB of {budget_mib} MiB budget"),
        None => "Latest: \u{2014}".to_owned(),
    }
}

/// Formats the parts running count as "healthy of total".
#[must_use]
pub fn format_parts_running(healthy: usize, total: usize) -> String {
    format!("{healthy} of {total}")
}

/// Formats the memory budget caption.
#[must_use]
pub fn format_budget_caption(budget_mib: u64) -> String {
    format!("of {budget_mib} MiB budget")
}

/// Formats the chain status header text and optional chip label & class.
#[must_use]
pub fn format_chain_status(
    chain: Option<&Result<ChainStatus, String>>,
) -> (Option<(&'static str, &'static str)>, String) {
    match chain {
        None => (None, "Checking\u{2026}".to_owned()),
        Some(Err(err)) => (Some(("ERROR", "down")), err.clone()),
        Some(Ok(cs)) if cs.ok => (
            Some(("INTACT", "ok")),
            format!("Audit log verified: {} records, chain intact", cs.checked),
        ),
        Some(Ok(_)) => (
            Some(("BROKEN", "down")),
            "Audit log chain is broken".to_owned(),
        ),
    }
}

/// Formats an audit row summary for accessibility and testing.
#[must_use]
pub fn format_audit_row_summary(row: &AuditRow) -> String {
    format!(
        "{}: {} ({} · {})",
        row.kind, row.skill, row.status, row.tier
    )
}

/// Formats the relative age of an audit event, or `None` if timestamps are absent.
#[must_use]
pub fn format_audit_time(now_ns: u64, at_ns: u64) -> Option<String> {
    if now_ns == 0 || at_ns == 0 {
        None
    } else {
        Some(format::age_label(now_ns, at_ns))
    }
}

fn build_audit_row(row: &AuditRow, now_ns: u64) -> gtk::Box {
    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row_box.add_css_class("row-line");
    row_box.add_css_class("audit-row");
    row_box.set_valign(gtk::Align::Center);
    row_box.update_property(
        &[gtk::accessible::Property::Label(&format_audit_row_summary(
            row,
        ))],
    );

    let icon_name = audit_row_icon(&row.kind, &row.status);
    let badge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    badge.add_css_class("badge-icon");
    badge.set_halign(gtk::Align::Center);
    badge.set_valign(gtk::Align::Center);
    badge.append(&icon(icon_name, 16));
    row_box.append(&badge);

    let skill_lbl = label(&row.skill, &["mono"]);
    skill_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    skill_lbl.set_hexpand(true);
    skill_lbl.set_xalign(0.0);
    row_box.append(&skill_lbl);

    if let Some(time_str) = format_audit_time(now_ns, row.at_ns) {
        let time_lbl = label(&time_str, &["caption", "num", "muted", "audit-time"]);
        row_box.append(&time_lbl);
    }

    let status_lbl = label(&row.status, &["caption"]);
    status_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    row_box.append(&status_lbl);

    let t_class = tier_class(&row.tier);
    let tier_lbl = if t_class.is_empty() {
        label(&row.tier, &["chip"])
    } else {
        label(&row.tier, &["chip", t_class])
    };
    tier_lbl.set_valign(gtk::Align::Center);
    row_box.append(&tier_lbl);

    row_box
}

/// The Activity page.
pub struct ActivityPage {
    root: gtk::ScrolledWindow,

    // Key tiles
    key_memory_val: gtk::Label,
    key_memory_bar: gtk::ProgressBar,
    key_memory_cap: gtk::Label,

    key_parts_val: gtk::Label,
    key_speed_val: gtk::Label,

    // Memory sparkline
    memory_spark: Spark,
    memory_spark_caption: gtk::Label,

    // Response times
    response_empty: gtk::Label,
    response_rows: gtk::Box,
    response_caption: gtk::Label,

    // Audit trail
    chain_chip: gtk::Label,
    chain_label: gtk::Label,
    audit_skeleton: gtk::Box,
    audit_error: gtk::Label,
    audit_empty: gtk::Label,
    audit_rows: gtk::Box,
}

impl ActivityPage {
    /// Builds the page.
    #[must_use]
    pub fn new(_ctx: &Ctx) -> Self {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 20);
        content.add_css_class("page");

        // Header
        let title_lbl = label("Activity", &["title-page"]);
        let greet_lbl = label(
            "How OctoPlus is doing right now and what it has done.",
            &["title-greet"],
        );
        greet_lbl.set_wrap(true);
        content.append(&title_lbl);
        content.append(&greet_lbl);

        // Key cards in a FlowBox (min 1 / max 3)
        let keys_row = gtk::FlowBox::new();
        keys_row.set_selection_mode(gtk::SelectionMode::None);
        keys_row.set_min_children_per_line(1);
        keys_row.set_max_children_per_line(3);
        keys_row.set_column_spacing(12);
        keys_row.set_row_spacing(12);
        keys_row.set_homogeneous(true);
        keys_row.set_can_focus(false);

        // Card 1: MEMORY IN USE
        let card_mem = card(&[]);
        card_mem.set_hexpand(true);
        card_mem.set_size_request(200, -1);
        card_mem.append(&label("MEMORY IN USE", &["title-card"]));
        let key_memory_val = label("\u{2014}", &["metric", "num"]);
        key_memory_val.set_xalign(0.0);
        card_mem.append(&key_memory_val);
        let key_memory_bar = bar();
        card_mem.append(&key_memory_bar);
        let key_memory_cap = label("\u{2014}", &["caption", "muted"]);
        key_memory_cap.set_xalign(0.0);
        card_mem.append(&key_memory_cap);
        keys_row.append(&card_mem);

        // Card 2: PARTS RUNNING
        let card_parts = card(&[]);
        card_parts.set_hexpand(true);
        card_parts.set_size_request(200, -1);
        card_parts.append(&label("PARTS RUNNING", &["title-card"]));
        let key_parts_val = label("\u{2014}", &["metric", "num"]);
        key_parts_val.set_xalign(0.0);
        card_parts.append(&key_parts_val);
        let key_parts_cap = label("healthy of total", &["caption", "muted"]);
        key_parts_cap.set_xalign(0.0);
        card_parts.append(&key_parts_cap);
        keys_row.append(&card_parts);

        // Card 3: MODEL SPEED
        let card_speed = card(&[]);
        card_speed.set_hexpand(true);
        card_speed.set_size_request(200, -1);
        card_speed.append(&label("MODEL SPEED", &["title-card"]));
        let key_speed_val = label("\u{2014}", &["metric", "num"]);
        key_speed_val.set_xalign(0.0);
        card_speed.append(&key_speed_val);
        let key_speed_cap = label("inference p99", &["caption", "muted"]);
        key_speed_cap.set_xalign(0.0);
        card_speed.append(&key_speed_cap);
        keys_row.append(&card_speed);

        content.append(&keys_row);

        // Memory sparkline card
        let mem_card = card(&[]);
        mem_card.append(&label("Memory, last 5 minutes", &["title-section"]));
        let memory_spark = Spark::new(72, "Memory usage over the last 5 minutes");
        mem_card.append(memory_spark.widget());
        let memory_spark_caption = label("Latest: \u{2014}", &["caption"]);
        mem_card.append(&memory_spark_caption);
        content.append(&mem_card);

        // Response times card
        let resp_card = card(&[]);
        resp_card.append(&label("Response times", &["title-section"]));
        let response_empty = label("No requests measured yet.", &["caption", "muted"]);
        resp_card.append(&response_empty);
        let response_rows = gtk::Box::new(gtk::Orientation::Vertical, 4);
        response_rows.set_visible(false);
        resp_card.append(&response_rows);
        let response_caption = label("Slowest 1% of requests per part.", &["caption", "muted"]);
        response_caption.set_visible(false);
        resp_card.append(&response_caption);
        content.append(&resp_card);

        // Audit trail card
        let audit_card = card(&[]);
        audit_card.append(&label("Audit trail", &["title-section"]));

        let chain_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        chain_box.set_valign(gtk::Align::Center);
        let chain_chip = label("INTACT", &["chip", "ok"]);
        chain_chip.set_visible(false);
        let chain_label = label("Checking\u{2026}", &["caption"]);
        chain_label.set_wrap(true);
        chain_box.append(&chain_chip);
        chain_box.append(&chain_label);
        audit_card.append(&chain_box);

        let audit_skeleton = gtk::Box::new(gtk::Orientation::Vertical, 8);
        audit_skeleton.append(&skeleton(280));
        audit_skeleton.append(&skeleton(240));
        audit_skeleton.append(&skeleton(200));
        audit_card.append(&audit_skeleton);

        let audit_error = label("", &["caption", "state-down"]);
        audit_error.set_visible(false);
        audit_card.append(&audit_error);

        let audit_empty = label("Nothing has been audited yet.", &["caption", "muted"]);
        audit_empty.set_visible(false);
        audit_card.append(&audit_empty);

        let audit_rows = gtk::Box::new(gtk::Orientation::Vertical, 4);
        audit_rows.set_visible(false);
        audit_card.append(&audit_rows);

        content.append(&audit_card);

        let root = gtk::ScrolledWindow::new();
        root.set_child(Some(&content));
        root.set_hscrollbar_policy(gtk::PolicyType::Automatic);

        Self {
            root,
            key_memory_val,
            key_memory_bar,
            key_memory_cap,
            key_parts_val,
            key_speed_val,
            memory_spark,
            memory_spark_caption,
            response_empty,
            response_rows,
            response_caption,
            chain_chip,
            chain_label,
            audit_skeleton,
            audit_error,
            audit_empty,
            audit_rows,
        }
    }

    fn redraw_health(&self, state: &AppState) {
        if !state.have_snapshot {
            self.key_memory_val.set_text("\u{2014}");
            set_bar(&self.key_memory_bar, 0.0);
            self.key_memory_cap.set_text("\u{2014}");
            self.key_parts_val.set_text("\u{2014}");
            self.key_speed_val.set_text("\u{2014}");
            self.memory_spark.set(Vec::new());
            self.memory_spark_caption.set_text("Latest: \u{2014}");
            self.response_empty.set_visible(true);
            self.response_caption.set_visible(false);
            self.response_rows.set_visible(false);
            return;
        }

        let snap = &state.snapshot;

        // Key tile 1: Memory
        let total_rss_bytes = snap.total_rss_mib * 1024 * 1024;
        self.key_memory_val
            .set_text(&format::bytes_iec(total_rss_bytes));
        let fraction = if snap.total_budget_mib > 0 {
            #[allow(clippy::cast_precision_loss)]
            let f = (snap.total_rss_mib as f64) / (snap.total_budget_mib as f64);
            f.clamp(0.0, 1.0)
        } else {
            0.0
        };
        set_bar(&self.key_memory_bar, fraction);
        self.key_memory_cap
            .set_text(&format_budget_caption(snap.total_budget_mib));

        // Key tile 2: Parts running
        self.key_parts_val
            .set_text(&format_parts_running(snap.healthy, snap.total));

        // Key tile 3: Model speed
        let speed = state.part(Part::Inference).map_or(0, |p| p.p99_ns);
        if speed > 0 {
            self.key_speed_val.set_text(&format::latency(speed));
        } else {
            self.key_speed_val.set_text("\u{2014}");
        }

        // Memory sparkline
        #[allow(clippy::cast_precision_loss)]
        let points = state
            .rss_total
            .normalized(0.0, snap.total_budget_mib as f64);
        self.memory_spark.set(points);
        self.memory_spark_caption
            .set_text(&format_memory_spark_caption(
                state.rss_total.last(),
                snap.total_budget_mib,
            ));

        // Response times
        let parts_with_p99: Vec<_> = snap.parts.iter().filter(|p| p.p99_ns > 0).collect();
        if parts_with_p99.is_empty() {
            self.response_empty.set_visible(true);
            self.response_caption.set_visible(false);
            self.response_rows.set_visible(false);
        } else {
            self.response_empty.set_visible(false);
            self.response_caption.set_visible(true);
            self.response_rows.set_visible(true);

            let mut max_p99 = 1;
            for p in &parts_with_p99 {
                if p.p99_ns > max_p99 {
                    max_p99 = p.p99_ns;
                }
            }

            while let Some(ch) = self.response_rows.first_child() {
                self.response_rows.remove(&ch);
            }

            for p in parts_with_p99 {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                row.add_css_class("row-line");
                row.set_valign(gtk::Align::Center);

                let title_lbl = label(p.title, &["body"]);
                title_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
                title_lbl.set_size_request(120, -1);
                title_lbl.set_xalign(0.0);
                row.append(&title_lbl);

                let b = bar();
                b.set_fraction(scale_p99(p.p99_ns, max_p99).clamp(0.0, 1.0));
                b.set_hexpand(true);
                b.set_size_request(80, -1);
                row.append(&b);

                let val_lbl = label(&format::latency(p.p99_ns), &["caption", "num"]);
                val_lbl.set_size_request(60, -1);
                val_lbl.set_xalign(1.0);
                row.append(&val_lbl);

                self.response_rows.append(&row);
            }
        }
    }

    fn redraw_ledgers(&self, state: &AppState) {
        // Hash-chain check
        let (chip_info, text) = format_chain_status(state.ledgers.chain.as_ref());
        if let Some((chip_text, chip_class)) = chip_info {
            self.chain_chip.set_text(chip_text);
            self.chain_chip.remove_css_class("ok");
            self.chain_chip.remove_css_class("down");
            self.chain_chip.add_css_class("chip");
            self.chain_chip.add_css_class(chip_class);
            self.chain_chip.set_visible(true);
        } else {
            self.chain_chip.set_visible(false);
        }
        self.chain_label.set_text(&text);
        let is_down = match state.ledgers.chain.as_ref() {
            Some(Err(_)) => true,
            Some(Ok(cs)) => !cs.ok,
            _ => false,
        };
        if is_down {
            self.chain_label.add_css_class("state-down");
        } else {
            self.chain_label.remove_css_class("state-down");
        }

        // Audit records
        match &state.ledgers.audit {
            None => {
                self.audit_skeleton.set_visible(true);
                self.audit_error.set_visible(false);
                self.audit_empty.set_visible(false);
                self.audit_rows.set_visible(false);
            }
            Some(Err(msg)) => {
                self.audit_skeleton.set_visible(false);
                self.audit_error.set_text(msg);
                self.audit_error.set_visible(true);
                self.audit_empty.set_visible(false);
                self.audit_rows.set_visible(false);
            }
            Some(Ok(rows)) if rows.is_empty() => {
                self.audit_skeleton.set_visible(false);
                self.audit_error.set_visible(false);
                self.audit_empty.set_visible(true);
                self.audit_rows.set_visible(false);
            }
            Some(Ok(rows)) => {
                self.audit_skeleton.set_visible(false);
                self.audit_error.set_visible(false);
                self.audit_empty.set_visible(false);
                self.audit_rows.set_visible(true);

                while let Some(ch) = self.audit_rows.first_child() {
                    self.audit_rows.remove(&ch);
                }

                let now_ns = state.snapshot.generated_at_ns;
                for row in rows.iter().take(12) {
                    let row_box = build_audit_row(row, now_ns);
                    self.audit_rows.append(&row_box);
                }
            }
        }
    }
}

impl Page for ActivityPage {
    fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    fn refresh(&self, state: &AppState, changed: Changed) {
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

    #[test]
    fn test_tier_class() {
        assert_eq!(tier_class("SAFE"), "ok");
        assert_eq!(tier_class("REVIEW"), "warn");
        assert_eq!(tier_class("DANGEROUS"), "down");
        assert_eq!(tier_class("OTHER"), "");
        assert_eq!(tier_class(""), "");
    }

    #[test]
    fn test_scale_p99() {
        assert!((scale_p99(0, 100) - 0.0).abs() < f64::EPSILON);
        assert!((scale_p99(50, 100) - 0.5).abs() < f64::EPSILON);
        assert!((scale_p99(100, 100) - 1.0).abs() < f64::EPSILON);
        assert!((scale_p99(150, 100) - 1.0).abs() < f64::EPSILON);
        assert!((scale_p99(100, 0) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_format_memory_spark_caption() {
        assert_eq!(format_memory_spark_caption(None, 6144), "Latest: \u{2014}");
        assert_eq!(
            format_memory_spark_caption(Some(3840.0), 6144),
            "Latest: 3840 MiB of 6144 MiB budget"
        );
    }

    #[test]
    fn test_format_parts_running() {
        assert_eq!(format_parts_running(8, 8), "8 of 8");
        assert_eq!(format_parts_running(7, 8), "7 of 8");
    }

    #[test]
    fn test_format_budget_caption() {
        assert_eq!(format_budget_caption(6144), "of 6144 MiB budget");
    }

    #[test]
    fn test_format_chain_status() {
        assert_eq!(
            format_chain_status(None),
            (None, "Checking\u{2026}".to_owned())
        );

        let err_res = Err("socket closed".to_owned());
        assert_eq!(
            format_chain_status(Some(&err_res)),
            (Some(("ERROR", "down")), "socket closed".to_owned())
        );

        let ok_res = Ok(ChainStatus {
            ok: true,
            checked: 42,
        });
        assert_eq!(
            format_chain_status(Some(&ok_res)),
            (
                Some(("INTACT", "ok")),
                "Audit log verified: 42 records, chain intact".to_owned()
            )
        );

        let broken_res = Ok(ChainStatus {
            ok: false,
            checked: 42,
        });
        assert_eq!(
            format_chain_status(Some(&broken_res)),
            (
                Some(("BROKEN", "down")),
                "Audit log chain is broken".to_owned()
            )
        );
    }

    #[test]
    fn test_format_audit_row_summary() {
        let row = AuditRow {
            seq: 1,
            at_ns: 1_000_000_000,
            kind: "ATTEMPT".to_owned(),
            skill: "file_read".to_owned(),
            status: "ok".to_owned(),
            tier: "SAFE".to_owned(),
        };
        assert_eq!(
            format_audit_row_summary(&row),
            "ATTEMPT: file_read (ok · SAFE)"
        );
    }

    #[test]
    fn test_format_audit_time() {
        assert_eq!(format_audit_time(0, 100), None);
        assert_eq!(format_audit_time(100, 0), None);
        assert_eq!(
            format_audit_time(10_000_000_000, 5_000_000_000),
            Some("5 s ago".to_owned())
        );
    }

    #[test]
    fn test_audit_row_icon() {
        assert_eq!(
            audit_row_icon("RESULT", "EXECUTED (ok)"),
            "octopit-check-symbolic"
        );
        assert_eq!(
            audit_row_icon("RESULT", "FAILED (error)"),
            "octopit-activity-symbolic"
        );
        assert_eq!(
            audit_row_icon("ATTEMPT", "STARTED"),
            "octopit-activity-symbolic"
        );
        assert_eq!(
            audit_row_icon("CALL", "EXECUTED"),
            "octopit-activity-symbolic"
        );
    }
}
