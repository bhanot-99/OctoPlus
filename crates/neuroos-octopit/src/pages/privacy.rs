//! The Privacy page: egress ledger, part isolation and audit verification (docs/design/octopit-ux.md §4.5).

use gtk::prelude::*;
use gtk4 as gtk;

use neuroos_common::paths::Part;
use neuroos_octopit_core::controller::{Command, Controller};
use neuroos_octopit_core::format;
use neuroos_octopit_core::gateway::ChainStatus;
use neuroos_octopit_core::state::{AppState, Changed};

use super::{Ctx, Page};
use crate::widgets::{card, dot, label, set_dot, skeleton, state_class, state_word};

/// Returns the summary sentence for the hero tile given the count of egress requests.
#[must_use]
pub fn egress_summary_sentence(count: usize) -> String {
    if count == 0 {
        "Nothing has left this machine. A request leaves only after you approve it in the dialog."
            .to_owned()
    } else {
        format!("{count} approved requests have left this machine. Each one is listed below.")
    }
}

/// Truncates text to `max_chars`, appending an ellipsis character if truncated.
#[must_use]
pub fn truncate_ellipsis(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}\u{2026}")
    } else {
        prefix
    }
}

/// Returns the badge text and CSS class for a part's network isolation chip.
#[must_use]
pub fn part_isolation_chip(part: Part) -> (&'static str, &'static str) {
    match part {
        Part::Fetcher => ("ONLY NETWORK DOOR", "web"),
        _ => ("NO NETWORK", "ok"),
    }
}

/// Formats egress transferred bytes as "out <bytes_iec> / in <bytes_iec>".
#[must_use]
pub fn format_transfer_bytes(bytes_out: u64, bytes_in: u64) -> String {
    format!(
        "out {} / in {}",
        format::bytes_iec(bytes_out),
        format::bytes_iec(bytes_in)
    )
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

struct IsolationRowWidgets {
    part: Part,
    dot: gtk::Box,
    state_label: gtk::Label,
}

/// The Privacy page.
pub struct PrivacyPage {
    root: gtk::ScrolledWindow,
    controller: Controller,

    // Hero
    hero_loading: gtk::Box,
    hero_error: gtk::Box,
    hero_error_msg: gtk::Label,
    hero_ok: gtk::Box,
    hero_metric: gtk::Label,
    hero_sentence: gtk::Label,

    // Network ledger
    ledger_empty: gtk::Label,
    ledger_rows: gtk::Box,

    // Isolation
    isolation_rows: Vec<IsolationRowWidgets>,

    // Audit log
    audit_chip: gtk::Label,
    audit_text: gtk::Label,
}

impl PrivacyPage {
    /// Builds the page.
    #[must_use]
    pub fn new(ctx: &Ctx) -> Self {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
        content.add_css_class("page");

        // Header
        content.append(&label("Privacy", &["title-page"]));
        content.append(&label(
            "What leaves this machine, and what keeps it safe.",
            &["caption"],
        ));

        // Big Hero Card
        let hero = card(&["raised", "hero"]);

        let hero_loading = gtk::Box::new(gtk::Orientation::Vertical, 8);
        hero_loading.append(&label("Checking the network ledger\u{2026}", &["caption"]));
        hero_loading.append(&skeleton(260));
        hero.append(&hero_loading);

        let hero_error = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let hero_error_title = label(
            "Cannot read the network ledger",
            &["title-section", "state-down"],
        );
        let hero_error_msg = label("", &["caption", "state-down"]);
        hero_error_msg.set_wrap(true);
        hero_error.append(&hero_error_title);
        hero_error.append(&hero_error_msg);
        hero_error.set_visible(false);
        hero.append(&hero_error);

        let hero_ok = gtk::Box::new(gtk::Orientation::Vertical, 4);
        hero_ok.append(&label("SENT TO THE NETWORK", &["title-card"]));
        let hero_metric = label("0 B", &["metric", "num"]);
        hero_metric.set_xalign(0.0);
        hero_ok.append(&hero_metric);
        let hero_sentence = label(
            "Nothing has left this machine. A request leaves only after you approve it in the dialog.",
            &["body"],
        );
        hero_sentence.set_wrap(true);
        hero_ok.append(&hero_sentence);
        hero_ok.set_visible(false);
        hero.append(&hero_ok);

        content.append(&hero);

        // Network Ledger Card
        let ledger_card = card(&[]);
        ledger_card.append(&label("Network ledger", &["title-section"]));

        let ledger_empty = label("No requests yet.", &["caption", "muted"]);
        ledger_card.append(&ledger_empty);

        let ledger_rows = gtk::Box::new(gtk::Orientation::Vertical, 4);
        ledger_rows.set_visible(false);
        ledger_card.append(&ledger_rows);

        content.append(&ledger_card);

        // Isolation Card
        let iso_card = card(&[]);
        iso_card.append(&label("Isolation", &["title-section"]));

        let iso_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let initial_parts = AppState::new().snapshot.parts;
        let mut isolation_rows = Vec::with_capacity(initial_parts.len());

        for p in initial_parts {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            row.add_css_class("row-line");
            row.set_valign(gtk::Align::Center);

            let d = dot(p.state);
            row.append(&d);

            let title_lbl = label(p.title, &["body"]);
            title_lbl.set_hexpand(true);
            row.append(&title_lbl);

            let (chip_text, chip_class) = part_isolation_chip(p.part);
            let chip_lbl = label(chip_text, &["chip", chip_class]);
            row.append(&chip_lbl);

            let state_lbl = label(
                state_word(p.state),
                &["caption", &format!("state-{}", state_class(p.state))],
            );
            state_lbl.set_size_request(64, -1);
            state_lbl.set_xalign(1.0);
            row.append(&state_lbl);

            iso_box.append(&row);
            isolation_rows.push(IsolationRowWidgets {
                part: p.part,
                dot: d,
                state_label: state_lbl,
            });
        }
        iso_card.append(&iso_box);

        let iso_cap = label(
            "Only the web fetcher can reach the network, and only for a request you approved.",
            &["caption", "muted"],
        );
        iso_cap.set_wrap(true);
        iso_card.append(&iso_cap);

        content.append(&iso_card);

        // Audit Log Card
        let audit_card = card(&[]);
        audit_card.append(&label("Audit log", &["title-section"]));

        let audit_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        audit_row.set_valign(gtk::Align::Center);

        let audit_chip = label("INTACT", &["chip", "ok"]);
        audit_chip.set_visible(false);
        audit_row.append(&audit_chip);

        let audit_text = label("Checking\u{2026}", &["body"]);
        audit_text.set_hexpand(true);
        audit_row.append(&audit_text);

        let check_btn = gtk::Button::with_label("Check again");
        check_btn.add_css_class("btn");
        check_btn.set_halign(gtk::Align::End);
        check_btn.update_property(&[gtk::accessible::Property::Label(
            "Check audit log chain again",
        )]);
        let ctrl = ctx.controller.clone();
        check_btn.connect_clicked(move |_| {
            let _ = ctrl.send(Command::RefreshLedgers);
        });
        audit_row.append(&check_btn);

        audit_card.append(&audit_row);
        content.append(&audit_card);

        // What OctoPit does Card
        let does_card = card(&[]);
        does_card.append(&label("What OctoPit does", &["title-section"]));

        for bullet in [
            "OctoPit only reads status and sends your typed questions.",
            "It cannot approve anything. Approvals happen only in the dialog.",
            "It never records or hears audio.",
        ] {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.set_valign(gtk::Align::Start);
            let dot_lbl = label("\u{2022}", &["caption", "muted", "bullet-dot"]);
            row.append(&dot_lbl);
            let b_lbl = label(bullet, &["body"]);
            b_lbl.set_wrap(true);
            row.append(&b_lbl);
            does_card.append(&row);
        }

        content.append(&does_card);

        let root = gtk::ScrolledWindow::new();
        root.set_child(Some(&content));
        root.set_hscrollbar_policy(gtk::PolicyType::Automatic);

        Self {
            root,
            controller: ctx.controller.clone(),
            hero_loading,
            hero_error,
            hero_error_msg,
            hero_ok,
            hero_metric,
            hero_sentence,
            ledger_empty,
            ledger_rows,
            isolation_rows,
            audit_chip,
            audit_text,
        }
    }

    fn redraw_ledgers(&self, state: &AppState) {
        // Big hero card
        match &state.ledgers.egress {
            None => {
                self.hero_loading.set_visible(true);
                self.hero_error.set_visible(false);
                self.hero_ok.set_visible(false);
            }
            Some(Err(msg)) => {
                self.hero_loading.set_visible(false);
                self.hero_error.set_visible(true);
                self.hero_ok.set_visible(false);
                self.hero_error_msg.set_text(msg);
            }
            Some(Ok(rows)) => {
                self.hero_loading.set_visible(false);
                self.hero_error.set_visible(false);
                self.hero_ok.set_visible(true);

                let sum_bytes_out: u64 = rows.iter().map(|r| r.bytes_out).sum();
                self.hero_metric.set_text(&format::bytes_iec(sum_bytes_out));
                self.hero_sentence
                    .set_text(&egress_summary_sentence(rows.len()));
            }
        }

        // Network ledger rows
        match &state.ledgers.egress {
            Some(Ok(rows)) if !rows.is_empty() => {
                self.ledger_empty.set_visible(false);
                self.ledger_rows.set_visible(true);

                while let Some(ch) = self.ledger_rows.first_child() {
                    self.ledger_rows.remove(&ch);
                }

                let now_ns = state.snapshot.generated_at_ns;
                for row in rows.iter().take(20) {
                    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                    row_box.add_css_class("row-line");
                    row_box.set_valign(gtk::Align::Center);

                    if now_ns > 0 && row.at_ns > 0 {
                        let age = format::age_label(now_ns, row.at_ns);
                        let time_lbl = label(&age, &["caption", "num", "ledger-time"]);
                        time_lbl.set_size_request(60, -1);
                        row_box.append(&time_lbl);
                    }

                    let host_lbl = label(&row.host, &["mono"]);
                    row_box.append(&host_lbl);

                    let clean_text = row.sent_text.replace('\n', " ");
                    let truncated = truncate_ellipsis(&clean_text, 80);
                    let text_lbl = label(&truncated, &["caption"]);
                    text_lbl.set_hexpand(true);
                    text_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    row_box.append(&text_lbl);

                    let transfer_text = format_transfer_bytes(row.bytes_out, row.bytes_in);
                    let transfer_lbl = label(&transfer_text, &["caption", "num"]);
                    transfer_lbl.set_halign(gtk::Align::End);
                    row_box.append(&transfer_lbl);

                    self.ledger_rows.append(&row_box);
                }
            }
            _ => {
                self.ledger_empty.set_visible(true);
                self.ledger_rows.set_visible(false);
            }
        }

        // Audit log card
        let (chip_info, text) = format_chain_status(state.ledgers.chain.as_ref());
        if let Some((chip_text, chip_class)) = chip_info {
            self.audit_chip.set_text(chip_text);
            self.audit_chip.remove_css_class("ok");
            self.audit_chip.remove_css_class("down");
            self.audit_chip.add_css_class("chip");
            self.audit_chip.add_css_class(chip_class);
            self.audit_chip.set_visible(true);
        } else {
            self.audit_chip.set_visible(false);
        }
        self.audit_text.set_text(&text);

        let is_down = match state.ledgers.chain.as_ref() {
            Some(Err(_)) => true,
            Some(Ok(cs)) => !cs.ok,
            _ => false,
        };
        if is_down {
            self.audit_text.add_css_class("state-down");
        } else {
            self.audit_text.remove_css_class("state-down");
        }
    }

    fn redraw_health(&self, state: &AppState) {
        for p in &state.snapshot.parts {
            if let Some(row) = self.isolation_rows.iter().find(|r| r.part == p.part) {
                set_dot(&row.dot, p.state);
                if state.have_snapshot {
                    row.state_label.set_text(state_word(p.state));
                } else {
                    row.state_label.set_text("\u{2014}");
                }
                for c in ["state-ok", "state-degraded", "state-down", "state-starting"] {
                    row.state_label.remove_css_class(c);
                }
                row.state_label
                    .add_css_class(&format!("state-{}", state_class(p.state)));
            }
        }
    }
}

impl Page for PrivacyPage {
    fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    fn refresh(&self, state: &AppState, changed: Changed) {
        if changed.ledgers {
            self.redraw_ledgers(state);
        }
        if changed.health {
            self.redraw_health(state);
        }
    }

    fn shown(&self) {
        let _ = self.controller.send(Command::RefreshLedgers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_egress_summary_sentence() {
        assert_eq!(
            egress_summary_sentence(0),
            "Nothing has left this machine. A request leaves only after you approve it in the dialog."
        );
        assert_eq!(
            egress_summary_sentence(1),
            "1 approved requests have left this machine. Each one is listed below."
        );
        assert_eq!(
            egress_summary_sentence(5),
            "5 approved requests have left this machine. Each one is listed below."
        );
    }

    #[test]
    fn test_truncate_ellipsis() {
        assert_eq!(truncate_ellipsis("hello", 10), "hello");
        assert_eq!(truncate_ellipsis("hello world", 5), "hello\u{2026}");
        assert_eq!(truncate_ellipsis("", 5), "");
        assert_eq!(truncate_ellipsis("exact", 5), "exact");
        assert_eq!(truncate_ellipsis("привет мир", 6), "привет\u{2026}");
    }

    #[test]
    fn test_part_isolation_chip() {
        assert_eq!(
            part_isolation_chip(Part::Fetcher),
            ("ONLY NETWORK DOOR", "web")
        );
        assert_eq!(part_isolation_chip(Part::Kernel), ("NO NETWORK", "ok"));
        assert_eq!(part_isolation_chip(Part::Storage), ("NO NETWORK", "ok"));
        assert_eq!(part_isolation_chip(Part::Inference), ("NO NETWORK", "ok"));
        assert_eq!(part_isolation_chip(Part::Voice), ("NO NETWORK", "ok"));
        assert_eq!(part_isolation_chip(Part::Monitor), ("NO NETWORK", "ok"));
        assert_eq!(part_isolation_chip(Part::Knowledge), ("NO NETWORK", "ok"));
        assert_eq!(part_isolation_chip(Part::Healthd), ("NO NETWORK", "ok"));
    }

    #[test]
    fn test_format_transfer_bytes() {
        assert_eq!(format_transfer_bytes(0, 0), "out 0 B / in 0 B");
        assert_eq!(
            format_transfer_bytes(1024, 2048),
            "out 1.0 KiB / in 2.0 KiB"
        );
    }

    #[test]
    fn test_format_chain_status() {
        assert_eq!(
            format_chain_status(None),
            (None, "Checking\u{2026}".to_owned())
        );

        let err_res = Err("connection refused".to_owned());
        assert_eq!(
            format_chain_status(Some(&err_res)),
            (Some(("ERROR", "down")), "connection refused".to_owned())
        );

        let ok_res = Ok(ChainStatus {
            ok: true,
            checked: 100,
        });
        assert_eq!(
            format_chain_status(Some(&ok_res)),
            (
                Some(("INTACT", "ok")),
                "Audit log verified: 100 records, chain intact".to_owned()
            )
        );

        let broken_res = Ok(ChainStatus {
            ok: false,
            checked: 100,
        });
        assert_eq!(
            format_chain_status(Some(&broken_res)),
            (
                Some(("BROKEN", "down")),
                "Audit log chain is broken".to_owned()
            )
        );
    }
}
