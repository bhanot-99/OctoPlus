//! `neuroos-synth-dump`: writes a *synthetic*, anonymised-format monitor dump for the Phase 4
//! noise gate and soak (D-26: real recordings come at integration). It models a working session
//! the way C1 would see it: coding with build bursts (hundreds of compiler subprocesses), browser
//! tabs with title churn, passive music, NeuroOS's own dialogs, idle gaps, health samples and a
//! few folder events. All text is `t_<hex>` placeholders or public app ids, exactly what a real
//! `--record` dump contains. Deterministic from `--seed`. Not shipped.

#![allow(clippy::print_stdout)] // a tool: its output is its result

use std::io::BufWriter;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use neuroos_proto::dump::DumpWriter;
use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{
    BatterySample, ChargeState, DiskSample, FileActivity, FileActivityKind, FocusChanged,
    IdleChanged, MediaChanged, MonitorEvent, PlaybackState, ProcessNode, ProcessTree,
    ResourceSample, ThermalSample,
};

#[derive(Debug, Parser)]
#[command(about = "Write a synthetic anonymised monitor dump")]
struct Args {
    /// Seed: the same seed gives the same dump.
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Length of the session in minutes.
    #[arg(long, default_value_t = 60)]
    minutes: u64,
    /// Output file (must not exist).
    #[arg(long)]
    out: PathBuf,
}

/// xorshift64*: small, deterministic, good enough for scenario noise.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[usize::try_from(self.below(v.len() as u64)).unwrap_or(0)]
    }
}

const SEC: u64 = 1_000_000_000;
const START: u64 = 1_700_000_000 * SEC;
const APPS: [&str; 6] = [
    "code",
    "org.gnome.Terminal",
    "firefox",
    "org.mozilla.firefox",
    "slack",
    "obsidian",
];

fn ph(rng: &mut Rng, pool: u64) -> String {
    format!(
        "t_{:012x}",
        rng.below(pool).wrapping_mul(0x9e37_79b9_7f4a_7c15) & 0xffff_ffff_ffff
    )
}

fn ev(at: u64, e: Event) -> MonitorEvent {
    MonitorEvent {
        observed_at_ns: at,
        event: Some(e),
    }
}

fn focus(at: u64, app: &str, title: String) -> MonitorEvent {
    ev(
        at,
        Event::Focus(FocusChanged {
            app_id: app.into(),
            title,
            pid: 4000,
            fullscreen: false,
            fullscreen_known: true,
        }),
    )
}

fn tree(at: u64, root: &str, kids: &[&str], extra: u32) -> MonitorEvent {
    let mut nodes = vec![ProcessNode {
        pid: 100,
        ppid: 1,
        name: root.into(),
    }];
    for (i, k) in kids.iter().enumerate() {
        nodes.push(ProcessNode {
            pid: 101 + i as u32,
            ppid: 100,
            name: (*k).into(),
        });
    }
    for j in 0..extra {
        nodes.push(ProcessNode {
            pid: 300 + j,
            ppid: 102,
            name: "cc1".into(),
        });
    }
    ev(
        at,
        Event::ProcessTree(ProcessTree {
            root_pid: 100,
            nodes,
        }),
    )
}

fn health(at: u64, rng: &mut Rng, out: &mut Vec<MonitorEvent>, tick: u64) {
    if tick.is_multiple_of(5) {
        out.push(ev(
            at,
            Event::Resources(ResourceSample {
                cpu_percent: 5.0 + rng.below(60) as f64,
                mem_total_bytes: 16_000_000_000,
                mem_available_bytes: 6_000_000_000 + rng.below(2_000_000_000),
                top_process: "firefox".into(),
                top_process_rss_bytes: 900_000_000,
            }),
        ));
    }
    if tick.is_multiple_of(10) {
        out.push(ev(
            at,
            Event::Thermal(ThermalSample {
                sensor: "k10temp".into(),
                celsius: 45.0 + rng.below(35) as f64,
            }),
        ));
    }
    if tick.is_multiple_of(60) {
        for (m, used) in [("/", 120_000_000_000_u64), ("/boot/efi", 500_000_000)] {
            out.push(ev(
                at,
                Event::Disk(DiskSample {
                    mount_point: m.into(),
                    total_bytes: 160_000_000_000,
                    used_bytes: used,
                    free_bytes: 160_000_000_000 - used,
                }),
            ));
        }
        out.push(ev(
            at,
            Event::Battery(BatterySample {
                percent: 70.0,
                state: ChargeState::Discharging as i32,
                time_estimate_s: 7200,
                time_estimate_known: true,
                on_ac: false,
            }),
        ));
    }
}

/// One build: a minute or two of process trees full of compiler subprocesses.
fn build(at: u64, rng: &mut Rng, out: &mut Vec<MonitorEvent>) {
    for i in 0..(10 + rng.below(20)) {
        let t = at + i * 5 * SEC;
        out.push(tree(
            t,
            "org.gnome.Terminal",
            &["bash", "cargo", "rustc", "cc1", "ld", "ninja", "make"],
            5 + rng.below(40) as u32,
        ));
    }
}

fn browsing(at: u64, rng: &mut Rng, out: &mut Vec<MonitorEvent>) -> u64 {
    let mut t = at;
    for _ in 0..(20 + rng.below(40)) {
        t += (1 + rng.below(3)) * SEC;
        out.push(focus(t, "firefox", ph(rng, 400)));
    }
    t
}

fn passive_music(at: u64, rng: &mut Rng, out: &mut Vec<MonitorEvent>) {
    let mut t = at;
    for _ in 0..6 {
        for state in [PlaybackState::Playing, PlaybackState::Paused] {
            out.push(ev(
                t,
                Event::Media(MediaChanged {
                    player: "spotify".into(),
                    state: state as i32,
                    title: ph(rng, 50),
                    artist: ph(rng, 20),
                }),
            ));
            t += (30 + rng.below(200)) * SEC;
        }
    }
}

fn video(at: u64, rng: &mut Rng, out: &mut Vec<MonitorEvent>) -> u64 {
    out.push(focus(at, "brave-browser", ph(rng, 30)));
    let mut t = at;
    for state in [
        PlaybackState::Playing,
        PlaybackState::Paused,
        PlaybackState::Playing,
        PlaybackState::Stopped,
    ] {
        t += (20 + rng.below(120)) * SEC;
        out.push(ev(
            t,
            Event::Media(MediaChanged {
                player: "brave".into(),
                state: state as i32,
                title: ph(rng, 30),
                artist: String::new(),
            }),
        ));
    }
    t + 10 * SEC
}

/// Alt-tabbing through many windows for a second each: the noise the promotion gate exists for.
fn flicker(at: u64, rng: &mut Rng, out: &mut Vec<MonitorEvent>) -> u64 {
    let mut t = at;
    for i in 0..45 {
        out.push(focus(
            t,
            &format!("com.example.popup{}", rng.below(1000) + i * 1000),
            ph(rng, 500),
        ));
        t += (1 + rng.below(2)) * SEC;
    }
    t
}

fn dialogs(at: u64, out: &mut Vec<MonitorEvent>) {
    out.push(focus(at, "org.neuroos.Confirm", "t_ownwindow0001".into()));
    out.push(tree(at + 2 * SEC, "neuroos-confirm", &["gtk-helper"], 0));
    out.push(focus(at + 8 * SEC, "code", "t_back0000001".into()));
}

fn folders(at: u64, rng: &mut Rng, out: &mut Vec<MonitorEvent>) {
    let note = format!("/home/t_user0000001/Notes/t_note{:07x}.md", rng.below(5));
    for (i, kind) in [
        FileActivityKind::Modified,
        FileActivityKind::Modified,
        FileActivityKind::Modified,
    ]
    .into_iter()
    .enumerate()
    {
        out.push(ev(
            at + i as u64 * 20 * SEC,
            Event::File(FileActivity {
                path: note.clone(),
                kind: kind as i32,
            }),
        ));
    }
    for name in ["HEAD", "index"] {
        out.push(ev(
            at + 3 * SEC,
            Event::File(FileActivity {
                path: format!("/home/t_user0000001/t_repo{:04x}/.git/{name}", rng.below(2)),
                kind: FileActivityKind::Modified as i32,
            }),
        ));
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut rng = Rng(args.seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let secs = args.minutes * 60;
    let mut events: Vec<MonitorEvent> = Vec::new();
    for tick in 0..secs {
        health(START + tick * SEC, &mut rng, &mut events, tick);
    }
    let mut t = START;
    let end = START + secs * SEC;
    events.push(ev(t, Event::Idle(IdleChanged { idle: false })));
    // Every session covers each situation at least once (the gate needs them all), then continues
    // at random. `block` runs one situation starting at `t` and returns when it ends.
    let mut block = 0u64;
    while t < end {
        let pick = if block < 11 { block } else { rng.below(11) };
        block += 1;
        match pick {
            0 | 1 => {
                // Coding: switch among editor, terminal and docs, a few seconds to a minute each.
                for _ in 0..(6 + rng.below(8)) {
                    let app = *rng.pick(&APPS);
                    events.push(focus(t, app, ph(&mut rng, 25)));
                    t += (2 + rng.below(80)) * SEC;
                }
            }
            2 | 4 => {
                build(t, &mut rng, &mut events);
                t += 120 * SEC;
            }
            3 | 5 => t = browsing(t, &mut rng, &mut events),
            6 => {
                passive_music(t, &mut rng, &mut events);
                t += 60 * SEC;
            }
            7 => t = video(t, &mut rng, &mut events),
            8 => {
                dialogs(t, &mut events);
                folders(t, &mut rng, &mut events);
                t += 60 * SEC;
            }
            10 => t = flicker(t, &mut rng, &mut events),
            _ => {
                // Away from the computer: 2 to 15 minutes.
                events.push(ev(t + 60 * SEC, Event::Idle(IdleChanged { idle: true })));
                t += (120 + rng.below(780)) * SEC;
                events.push(ev(t, Event::Idle(IdleChanged { idle: false })));
            }
        }
        t += 5 * SEC;
    }
    events.retain(|e| e.observed_at_ns < end);
    events.sort_by_key(|e| e.observed_at_ns);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args.out)
        .context("the output file must not exist")?;
    let mut w = DumpWriter::new(BufWriter::new(file))?;
    for e in &events {
        w.write(e)?;
    }
    let n = w.finish()?;
    println!("wrote {n} events to {}", args.out.display());
    Ok(())
}
