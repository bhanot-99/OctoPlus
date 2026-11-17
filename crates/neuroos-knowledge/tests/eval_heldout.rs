#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

//! KPI-13 (turn routing ≥ 90%) and KPI-2 ("this" resolved ≥ 95%) on sets that were written
//! before the router and the deictic parser were looked at for them, and that are not used to
//! tune either (`tests/eval/p5/*_heldout.jsonl`, committed in the same commit as this test).
//! A miss found here is fixed by adding a *regression fixture* elsewhere, never by editing
//! these files. Run with `--nocapture` to see every miss.

use neuroos_knowledge::deictic::{Anchor, parse};
use neuroos_knowledge::router::{RouteInput, RouteKind, route};
use serde::Deserialize;

/// First-run scores, each recorded before anything was tuned on that set (the router was
/// generalised from the misses of one set before the next was written):
///
/// | set | KPI-13 routing (needs 90%) | KPI-2 "this" (needs 95%) |
/// | :-: | :-: | :-: |
/// | A | 75.0% (60/80) | 97.5% (39/40) |
/// | B | 80.0% (68/85) | 90.0% (36/40) |
/// | C | 81.2% (65/80) | 87.5% (35/40) |
/// | D | 81.2% (65/80) | 97.5% (39/40) |
///
/// After BUG-070 work (router generalised from E, F, G in turn; sets E-G written by agy, blind):
///
/// | set | KPI-13 first run | router version |
/// | :-: | :-: | :- |
/// | E | 76.2% (80/105) | before the BUG-070 changes |
/// | F | 79.0% (83/105) | after tuning on E |
/// | G | 86.7% (91/105) | after tuning on E and F |
/// | H | 94.3% (99/105) | after tuning on E, F and G (target 90%: met) |
///
/// So on wording the router had not seen, KPI-13 was about 80% (not met); after BUG-070 it is
/// about 94% on a fresh blind set. KPI-2 on unseen wording is still about 93% (not part of BUG-070).
/// The sets all pass here only because they were used for tuning; they guard against
/// regressions. See `reports/phase-05-knowledge.md`.
const ROUTER_SET_A: &str = include_str!("../../../tests/eval/p5/router_heldout.jsonl");
const DEICTIC_SET_A: &str = include_str!("../../../tests/eval/p5/deictic_heldout.jsonl");
const ROUTER_SET_C: &str = include_str!("../../../tests/eval/p5/router_heldout_c.jsonl");
const DEICTIC_SET_C: &str = include_str!("../../../tests/eval/p5/deictic_heldout_c.jsonl");
const ROUTER_SET_D: &str = include_str!("../../../tests/eval/p5/router_heldout_d.jsonl");
const DEICTIC_SET_D: &str = include_str!("../../../tests/eval/p5/deictic_heldout_d.jsonl");
const ROUTER_SET_E: &str = include_str!("../../../tests/eval/p5/router_heldout_e.jsonl");
const ROUTER_SET_F: &str = include_str!("../../../tests/eval/p5/router_heldout_f.jsonl");
const ROUTER_SET_G: &str = include_str!("../../../tests/eval/p5/router_heldout_g.jsonl");
const ROUTER_SET_H: &str = include_str!("../../../tests/eval/p5/router_heldout_h.jsonl");
const ROUTER_SET_B: &str = include_str!("../../../tests/eval/p5/router_heldout_b.jsonl");
const DEICTIC_SET_B: &str = include_str!("../../../tests/eval/p5/deictic_heldout_b.jsonl");

#[derive(Deserialize)]
struct RouterRow {
    text: String,
    expected: String,
    nudge_window_open: bool,
    best_score: Option<f64>,
}

#[derive(Deserialize)]
struct DeicticRow {
    text: String,
    deictic: bool,
    anchor: String,
}

fn kind(s: &str) -> RouteKind {
    match s {
        "nudge_reply" => RouteKind::NudgeReply,
        "action" => RouteKind::Action,
        "desktop" => RouteKind::Desktop,
        "general" => RouteKind::General,
        other => panic!("unknown route {other}"),
    }
}

fn anchor(s: &str) -> Anchor {
    if s == "now" {
        Anchor::Now
    } else if s == "none" {
        Anchor::None
    } else if let Some(n) = s.strip_prefix("seconds_ago:") {
        Anchor::SecondsAgo(n.parse().unwrap())
    } else if let Some(n) = s.strip_prefix("range:") {
        Anchor::Range(n.parse().unwrap())
    } else {
        panic!("unknown anchor {s}")
    }
}

fn router_accuracy(name: &str, set: &str) -> f64 {
    let rows: Vec<RouterRow> = set
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(rows.len() >= 60, "the KPI needs a real set");
    println!("-- router set {name}");
    let mut right = 0;
    for r in &rows {
        let got = route(&RouteInput {
            text: &r.text,
            nudge_window_open: r.nudge_window_open,
            retrieval_best_score: r.best_score,
            cutoff: 0.6,
        })
        .route;
        if got == kind(&r.expected) {
            right += 1;
        } else {
            println!("ROUTER MISS {:?}: want {} got {got:?}", r.text, r.expected);
        }
    }
    let pct = 100.0 * f64::from(right) / rows.len() as f64;
    println!("KPI-13 set {name}: {right}/{} = {pct:.1}%", rows.len());
    pct
}

#[test]
fn kpi_13_turn_routing_on_the_held_out_sets() {
    // A is a regression set now; B is the measurement (first run recorded in the phase report).
    assert!(router_accuracy("A", ROUTER_SET_A) >= 90.0);
    assert!(router_accuracy("B", ROUTER_SET_B) >= 90.0);
    assert!(router_accuracy("C", ROUTER_SET_C) >= 90.0);
    assert!(
        router_accuracy("D", ROUTER_SET_D) >= 90.0,
        "KPI-13 below 90% on set D"
    );
    // E, F and G were written blind by a different author (agy) and run once before any tuning
    // on them; they are regression sets now (first runs in the table above).
    assert!(router_accuracy("E", ROUTER_SET_E) >= 90.0);
    assert!(router_accuracy("F", ROUTER_SET_F) >= 90.0);
    assert!(router_accuracy("G", ROUTER_SET_G) >= 90.0);
    // H is the final exam of the BUG-070 work: first run 94.3% (99/105) before anything was
    // changed for it, then its six misses were fixed and it became a regression set too.
    assert!(router_accuracy("H", ROUTER_SET_H) >= 90.0);
}

fn deictic_accuracy(name: &str, set: &str) -> f64 {
    let rows: Vec<DeicticRow> = set
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(rows.len() >= 40);
    let (mut flag_ok, mut both_ok) = (0, 0);
    for r in &rows {
        let got = parse(&r.text);
        let f = got.deictic == r.deictic;
        let a = got.anchor == anchor(&r.anchor);
        flag_ok += i32::from(f);
        both_ok += i32::from(f && a);
        if !(f && a) {
            println!(
                "DEICTIC MISS {:?}: want ({}, {}) got ({}, {:?})",
                r.text, r.deictic, r.anchor, got.deictic, got.anchor
            );
        }
    }
    let n = rows.len() as f64;
    println!(
        "KPI-2 set {name}: deictic flag {flag_ok}/{} = {:.1}%, flag and anchor {both_ok}/{} = {:.1}%",
        rows.len(),
        100.0 * f64::from(flag_ok) / n,
        rows.len(),
        100.0 * f64::from(both_ok) / n
    );
    100.0 * f64::from(both_ok) / n
}

#[test]
fn kpi_2_this_is_resolved_on_the_held_out_sets() {
    assert!(deictic_accuracy("A", DEICTIC_SET_A) >= 95.0);
    assert!(deictic_accuracy("B", DEICTIC_SET_B) >= 95.0);
    assert!(deictic_accuracy("C", DEICTIC_SET_C) >= 95.0);
    assert!(
        deictic_accuracy("D", DEICTIC_SET_D) >= 95.0,
        "KPI-2 below 95% on set D"
    );
}
