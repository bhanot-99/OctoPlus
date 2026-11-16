//! Domain adapters (FR-STO-02, Architecture §7.3): fixed templates and classification. Chunk text
//! comes from templates, never from a model, so what is stored is predictable and testable.

use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{Domain, MonitorEvent};

use super::HealthPoint;

/// Text for a window the user spent time in.
#[must_use]
pub fn focus_text(app_id: &str, title: &str) -> String {
    if title.is_empty() {
        format!("Used {app_id}")
    } else {
        format!("Used {app_id}: {title}")
    }
}

/// Text for the user coming back after being away.
#[must_use]
pub fn presence_text(away_ns: u64) -> String {
    format!(
        "Away from the computer for about {} minutes",
        away_ns / 60_000_000_000
    )
}

/// Text for a media change the user took part in.
#[must_use]
pub fn media_text(player: &str, state: &str, title: &str, artist: &str) -> String {
    match (title.is_empty(), artist.is_empty()) {
        (true, _) => format!("{player} is {state}"),
        (false, true) => format!("{player} {state}: {title}"),
        (false, false) => format!("{player} {state}: {title} by {artist}"),
    }
}

/// Text for activity in a process family.
#[must_use]
pub fn process_text(root: &str) -> String {
    format!("Running {root} and its helper processes")
}

/// Text for a build seen under `root`; `tools` are names from the build list only.
#[must_use]
pub fn build_text(root: &str, tools: &[String]) -> String {
    format!("Build activity in {root} using {}", tools.join(", "))
}

/// Where a file event belongs and what it is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileClass {
    /// Domain (git, notes or calendar).
    pub domain: Domain,
    /// Entity kind.
    pub kind: &'static str,
    /// Entity label: the repository folder, or the file name.
    pub label: String,
}

/// Classifies a path from C1's folder sensor: `.git` inside a path means a repository, `.ics` a
/// calendar file, `.md`/`.txt`/`.org` a note. Anything else is not stored.
#[must_use]
pub fn classify_file(path: &str) -> Option<FileClass> {
    let comps: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    if let Some(i) = comps.iter().position(|c| *c == ".git") {
        let label = comps.get(i.checked_sub(1)?)?;
        return Some(FileClass {
            domain: Domain::GitActivity,
            kind: "repo",
            label: (*label).to_owned(),
        });
    }
    let name = comps.last()?;
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    let (domain, kind) = match ext.as_str() {
        "ics" => (Domain::Calendar, "calendar"),
        "md" | "markdown" | "txt" | "org" => (Domain::Notes, "note"),
        _ => return None,
    };
    Some(FileClass {
        domain,
        kind,
        label: (*name).to_owned(),
    })
}

/// Text for a changed file or repository.
#[must_use]
pub fn file_text(class: &FileClass, verb: &str) -> String {
    match class.domain {
        Domain::GitActivity => format!("Git activity in repository {}", class.label),
        Domain::Calendar => format!("Calendar file {} {verb}", class.label),
        _ => format!("Note {} {verb}", class.label),
    }
}

/// Numeric readings from a health-type event (FR-MON-04..07); empty for other events. These
/// bypass the promotion gate and go to `health_samples`.
#[must_use]
pub fn health_points(ev: &MonitorEvent) -> Vec<HealthPoint> {
    let at = ev.observed_at_ns;
    let p = |metric: String, value: f64| HealthPoint {
        metric,
        value,
        at_ns: at,
    };
    match &ev.event {
        Some(Event::Resources(r)) => {
            let mut v = vec![p("cpu_percent".into(), r.cpu_percent)];
            if r.mem_total_bytes > 0 {
                let used = r.mem_total_bytes.saturating_sub(r.mem_available_bytes);
                #[allow(clippy::cast_precision_loss)]
                v.push(p(
                    "mem_used_percent".into(),
                    used as f64 / r.mem_total_bytes as f64 * 100.0,
                ));
            }
            v
        }
        Some(Event::Disk(d)) if d.total_bytes > 0 => {
            #[allow(clippy::cast_precision_loss)]
            let pct = d.used_bytes as f64 / d.total_bytes as f64 * 100.0;
            vec![p(format!("disk_used_percent:{}", d.mount_point), pct)]
        }
        Some(Event::Battery(b)) => {
            let mut v = vec![
                p("battery_percent".into(), b.percent),
                p("on_ac".into(), f64::from(u8::from(b.on_ac))),
            ];
            if b.time_estimate_known {
                #[allow(clippy::cast_precision_loss)]
                v.push(p("battery_time_s".into(), b.time_estimate_s as f64));
            }
            v
        }
        Some(Event::Thermal(t)) => vec![p(format!("thermal_c:{}", t.sensor), t.celsius)],
        _ => Vec::new(),
    }
}
