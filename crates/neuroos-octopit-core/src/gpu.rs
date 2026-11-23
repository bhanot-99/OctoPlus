//! GPU readings for the Backends page (`docs/design/octopit-ux.md` §4.3).
//!
//! The spike (Q-1) found that C1 must not read the GPU: NVIDIA's counters need the driver's
//! device files, which C1's sandbox rightly hides. OctoPit is an ordinary user program, so it asks
//! the vendor tool itself, with fixed arguments, read-only, with a short deadline. Nothing here
//! touches the network or any NeuroOS data.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The tool's full path: no search of `PATH`, so a look-alike earlier on the path is not run.
const NVIDIA_SMI: &str = "/usr/bin/nvidia-smi";

const QUERY: &str = "name,driver_version,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw,power.limit";

/// How long the tool may take before it is stopped.
const DEADLINE: Duration = Duration::from_millis(1500);

/// The most output read from the tool.
const MAX_OUTPUT: u64 = 4096;

/// One reading. A value the card does not report is `None`, shown as a dash.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuReading {
    /// Marketing name of the card.
    pub name: String,
    /// Driver version.
    pub driver: String,
    /// Busy percent, `0..=100`.
    pub utilization_pct: Option<f64>,
    /// Video memory in use, MiB.
    pub memory_used_mib: Option<f64>,
    /// Video memory in total, MiB.
    pub memory_total_mib: Option<f64>,
    /// Temperature in degrees Celsius.
    pub temperature_c: Option<f64>,
    /// Power draw in watts.
    pub power_w: Option<f64>,
    /// Power limit in watts.
    pub power_limit_w: Option<f64>,
}

impl GpuReading {
    /// Video memory in use, as a fraction `0.0..=1.0`.
    #[must_use]
    pub fn memory_fraction(&self) -> Option<f64> {
        match (self.memory_used_mib, self.memory_total_mib) {
            (Some(u), Some(t)) if t > 0.0 => Some((u / t).clamp(0.0, 1.0)),
            _ => None,
        }
    }
}

/// What the page shows.
#[derive(Debug, Clone, PartialEq)]
pub enum GpuState {
    /// A card answered.
    Present(GpuReading),
    /// No supported GPU tool or card was found: the page says so plainly.
    NotAvailable,
}

fn number(field: &str) -> Option<f64> {
    let f = field.trim();
    if f.is_empty() || f.starts_with('[') {
        return None;
    }
    f.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0)
}

/// Reads the first line of `nvidia-smi --query-gpu=... --format=csv,noheader,nounits`.
#[must_use]
pub fn parse_nvidia_smi(output: &str) -> Option<GpuReading> {
    let line = output.lines().find(|l| !l.trim().is_empty())?;
    let fields: Vec<&str> = line.split(',').collect();
    if fields.len() != 8 {
        return None;
    }
    let name: String = fields[0]
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(80)
        .collect();
    if name.is_empty() {
        return None;
    }
    Some(GpuReading {
        name,
        driver: fields[1]
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(24)
            .collect(),
        utilization_pct: number(fields[2]).map(|v| v.min(100.0)),
        memory_used_mib: number(fields[3]),
        memory_total_mib: number(fields[4]),
        temperature_c: number(fields[5]),
        power_w: number(fields[6]),
        power_limit_w: number(fields[7]),
    })
}

/// Asks the GPU tool once. Blocks for at most about 1.5 s: call it from a worker thread.
#[must_use]
pub fn sample() -> GpuState {
    let Ok(mut child) = Command::new(NVIDIA_SMI)
        .arg(format!("--query-gpu={QUERY}"))
        .arg("--format=csv,noheader,nounits")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_clear()
        .spawn()
    else {
        return GpuState::NotAvailable;
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return GpuState::NotAvailable;
                }
                break;
            }
            Ok(None) if started.elapsed() < DEADLINE => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return GpuState::NotAvailable;
            }
        }
    }
    let mut text = String::new();
    if let Some(out) = child.stdout.take()
        && out.take(MAX_OUTPUT).read_to_string(&mut text).is_err()
    {
        return GpuState::NotAvailable;
    }
    parse_nvidia_smi(&text).map_or(GpuState::NotAvailable, GpuState::Present)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normal_line_is_read() {
        let r = parse_nvidia_smi(
            "NVIDIA GeForce RTX 3050 Laptop GPU, 595.104.02, 4, 1, 4096, 46, 6.10, [N/A]\n",
        )
        .expect("parsed");
        assert_eq!(r.name, "NVIDIA GeForce RTX 3050 Laptop GPU");
        assert_eq!(r.driver, "595.104.02");
        assert_eq!(r.utilization_pct, Some(4.0));
        assert_eq!(r.memory_total_mib, Some(4096.0));
        assert_eq!(r.temperature_c, Some(46.0));
        assert_eq!(r.power_w, Some(6.1));
        assert_eq!(r.power_limit_w, None, "[N/A] is a dash, not zero");
        assert!((r.memory_fraction().unwrap_or(9.0) - 1.0 / 4096.0).abs() < 1e-9);
    }

    #[test]
    fn bad_output_is_refused() {
        assert_eq!(parse_nvidia_smi(""), None);
        assert_eq!(parse_nvidia_smi("only,three,fields"), None);
        assert_eq!(parse_nvidia_smi(", d, 1, 1, 1, 1, 1, 1"), None, "no name");
        assert_eq!(
            parse_nvidia_smi("NVIDIA-SMI has failed because it couldn't communicate"),
            None
        );
    }

    #[test]
    fn numbers_are_clamped_and_never_negative_or_infinite() {
        let r = parse_nvidia_smi("G, d, 250, -3, inf, nan, 1e999, 5").expect("parsed");
        assert_eq!(r.utilization_pct, Some(100.0));
        assert_eq!(r.memory_used_mib, None);
        assert_eq!(r.memory_total_mib, None);
        assert_eq!(r.temperature_c, None);
        assert_eq!(r.power_w, None);
        assert_eq!(r.power_limit_w, Some(5.0));
    }

    #[test]
    fn memory_fraction_needs_both_numbers() {
        let mut r = parse_nvidia_smi("G, d, 1, 2048, 4096, 1, 1, 1").expect("parsed");
        assert_eq!(r.memory_fraction(), Some(0.5));
        r.memory_total_mib = Some(0.0);
        assert_eq!(r.memory_fraction(), None);
    }

    #[test]
    fn a_name_cannot_carry_control_characters() {
        let r = parse_nvidia_smi("Evil\u{1b}[31m Card, d, 1, 1, 1, 1, 1, 1").expect("parsed");
        assert!(!r.name.contains('\u{1b}'));
    }
}
