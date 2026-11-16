//! W3 / P3-S03: CPU, memory, disk, battery and temperature samplers (FR-MON-04..07).
//! Parsers are tested on fixture text, sensors on a fake `/proc` and `/sys` tree, and each
//! sensor has a negative control. `local_` tests read the real machine once.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp
)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use neuroos_common::config::MonitorConfig;
use neuroos_health::Health;
use neuroos_monitor::bus::{Bus, PassGate, Subscription};
use neuroos_monitor::clock::{Clock, FakeClock};
use neuroos_monitor::sensor::Sensor;
use neuroos_monitor::sensors::battery::{self, BatterySensor, Reading};
use neuroos_monitor::sensors::disk::{self, DiskSensor};
use neuroos_monitor::sensors::resources::{self, CpuTimes, ResourcesSensor};
use neuroos_monitor::sensors::thermal::{self, Hwmon, ThermalSensor};
use neuroos_monitor::supervise;
use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{ChargeState, MonitorEvent};
use proptest::prelude::*;

fn start<S: Sensor>(
    sensor: S,
) -> (
    Subscription,
    tokio::sync::watch::Sender<bool>,
    tokio::task::JoinHandle<()>,
) {
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(1_000));
    let health = Health::new("neuroos-monitor", "test");
    let bus = Bus::new(
        64,
        Arc::new(PassGate),
        Arc::clone(&clock),
        Arc::clone(&health),
    );
    let sub = bus.subscribe();
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(supervise(sensor, bus, clock, health, stop_rx));
    (sub, stop_tx, task)
}

async fn next(sub: &Subscription, secs: u64) -> Option<MonitorEvent> {
    tokio::time::timeout(Duration::from_secs(secs), sub.next())
        .await
        .ok()
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn cfg() -> MonitorConfig {
    MonitorConfig::default()
}

// ---------------------------------------------------------------- resources (FR-MON-04)

const STAT1: &str = "cpu  100 0 100 700 100 0 0 0 0 0\ncpu0 1 2 3\n";
const STAT2: &str = "cpu  200 0 200 750 150 0 0 0 0 0\n";
const MEMINFO: &str = "MemTotal:       16000000 kB\nMemFree: 1 kB\nMemAvailable:    8000000 kB\n";

#[test]
fn cpu_times_and_percent_follow_the_proc_stat_rules() {
    let a = resources::parse_cpu_times(STAT1).unwrap();
    assert_eq!((a.busy, a.total), (200, 1000));
    let b = resources::parse_cpu_times(STAT2).unwrap();
    // busy 400-200 = 200 over total 1300-1000 = 300
    assert!((resources::cpu_percent(a, b).unwrap() - 66.666_666).abs() < 0.001);
    assert_eq!(resources::cpu_percent(a, a), None, "no time passed");
    assert_eq!(
        resources::cpu_percent(b, a),
        None,
        "counters went backwards"
    );
    let wrap = CpuTimes {
        busy: 0,
        total: 5_000,
    };
    assert_eq!(resources::cpu_percent(b, wrap), None);
    for bad in [
        "",
        "cpu0 1 2 3",
        "cpu  1 2",
        "cpu  a b c d e f g h",
        "mem 1 2 3 4 5",
    ] {
        assert_eq!(resources::parse_cpu_times(bad), None, "{bad:?}");
    }
}

#[test]
fn meminfo_gives_bytes_and_rejects_nonsense() {
    assert_eq!(
        resources::parse_meminfo(MEMINFO),
        Some((16_000_000 * 1024, 8_000_000 * 1024))
    );
    assert_eq!(
        resources::parse_meminfo("MemTotal: 1 kB\n"),
        None,
        "no MemAvailable"
    );
    assert_eq!(
        resources::parse_meminfo("MemTotal: 1 kB\nMemAvailable: 9 kB\n"),
        None,
        "available > total"
    );
    assert_eq!(
        resources::parse_meminfo("MemTotal: x kB\nMemAvailable: 1 kB\n"),
        None
    );
    assert_eq!(
        resources::parse_meminfo(&format!("MemTotal: {} kB\nMemAvailable: 1 kB\n", u64::MAX)),
        None,
        "overflow"
    );
}

fn fake_proc(root: &Path, stat: &str) {
    write(root, "proc/stat", stat);
    write(root, "proc/meminfo", MEMINFO);
    write(root, "proc/100/statm", "1000 250 0 0 0 0 0\n");
    write(root, "proc/100/comm", "smallapp\n");
    write(root, "proc/200/statm", "9000 4096 0 0 0 0 0\n");
    write(root, "proc/200/comm", "bigapp-with-lo\n");
    write(root, "proc/300/comm", "no-statm-vanished\n");
    write(root, "proc/self/statm", "1 99999 0 0 0 0 0\n"); // not a pid: must be ignored
}

#[tokio::test(start_paused = true)]
async fn resources_sensor_baselines_first_then_emits_with_the_biggest_process() {
    let dir = tempfile::tempdir().unwrap();
    fake_proc(dir.path(), STAT1);
    let (sub, stop, task) = start(ResourcesSensor::with_root(&cfg(), dir.path()));
    assert!(
        next(&sub, 2).await.is_none(),
        "the first tick is only a baseline"
    );
    write(dir.path(), "proc/stat", STAT2);
    let ev = next(&sub, 10).await.expect("second tick emits");
    let Some(Event::Resources(r)) = ev.event else {
        panic!("resources expected")
    };
    assert!((r.cpu_percent - 66.67).abs() < 0.01);
    assert_eq!(
        (r.mem_total_bytes, r.mem_available_bytes),
        (16_000_000 * 1024, 8_000_000 * 1024)
    );
    assert_eq!(r.top_process, "bigapp-with-lo");
    assert_eq!(
        r.top_process_rss_bytes,
        4096 * rustix::param::page_size() as u64
    );
    stop.send(true).unwrap();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn negative_control_unreadable_stat_degrades_instead_of_emitting_garbage() {
    let dir = tempfile::tempdir().unwrap(); // no proc/stat at all
    let (sub, stop, task) = start(ResourcesSensor::with_root(&cfg(), dir.path()));
    assert!(next(&sub, 30).await.is_none());
    stop.send(true).unwrap();
    task.await.unwrap();
}

// ---------------------------------------------------------------- disk (FR-MON-05)

const MOUNTINFO: &str = "\
22 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw
23 22 0:5 / /proc rw - proc proc rw
24 22 0:6 / /dev/shm rw - tmpfs tmpfs rw
25 22 259:1 / /boot/efi rw - vfat /dev/nvme0n1p1 rw
26 22 259:2 /home/u/data /mnt/bind rw - ext4 /dev/nvme0n1p2 rw
27 22 8:17 / /media/u/My\\040Drive rw - exfat /dev/sdb1 rw
28 22 0:40 / /snap/core/1 ro - squashfs /dev/loop0 ro
29 22 0:41 / /run/user/1000 rw - tmpfs tmpfs rw
30 22 259:2 / / rw - ext4 /dev/nvme0n1p2 rw
garbage line without separator
";

#[test]
fn mountinfo_keeps_only_user_filesystems_once_and_decodes_escapes() {
    let points: Vec<String> = disk::relevant_mounts(MOUNTINFO)
        .into_iter()
        .map(|m| m.mount_point)
        .collect();
    assert_eq!(points, vec!["/", "/boot/efi", "/media/u/My Drive"]);
    assert_eq!(disk::decode_octal(r"a\040b\011c\134d"), "a b\tc\\d");
    assert_eq!(disk::decode_octal(r"\04"), r"\04", "short escapes stay");
    assert_eq!(disk::decode_octal(r"\999"), r"\999", "non-octal stays");
    assert!(disk::parse_mountinfo("").is_empty());
}

#[tokio::test(start_paused = true)]
async fn disk_sensor_reports_total_used_free_for_a_real_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mi = dir.path().join("mountinfo");
    std::fs::write(
        &mi,
        format!(
            "22 1 259:2 / {} rw - ext4 /dev/x rw\n",
            dir.path().display()
        ),
    )
    .unwrap();
    let (sub, stop, task) = start(DiskSensor::with_mountinfo(&cfg(), &mi));
    let ev = next(&sub, 5).await.expect("first tick emits at once");
    let Some(Event::Disk(d)) = ev.event else {
        panic!("disk expected")
    };
    assert_eq!(d.mount_point, dir.path().display().to_string());
    assert!(d.total_bytes > 0 && d.used_bytes <= d.total_bytes && d.free_bytes <= d.total_bytes);
    stop.send(true).unwrap();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn negative_control_a_mount_that_cannot_be_read_is_skipped_and_a_missing_file_degrades() {
    let dir = tempfile::tempdir().unwrap();
    let mi = dir.path().join("mountinfo");
    std::fs::write(
        &mi,
        "22 1 259:2 / /definitely/not/mounted rw - ext4 /dev/x rw\n",
    )
    .unwrap();
    let (sub, stop, task) = start(DiskSensor::with_mountinfo(&cfg(), &mi));
    assert!(next(&sub, 5).await.is_none(), "statvfs fails: no event");
    stop.send(true).unwrap();
    task.await.unwrap();
    let (sub, stop, task) = start(DiskSensor::with_mountinfo(
        &cfg(),
        &dir.path().join("absent"),
    ));
    assert!(next(&sub, 5).await.is_none());
    stop.send(true).unwrap();
    task.await.unwrap();
}

// ---------------------------------------------------------------- battery (FR-MON-06)

fn bat(status: &str, now: u64, full: u64, power: u64) -> Reading {
    Reading {
        capacity: Some(50.0),
        status: Some(status.into()),
        energy_now: Some(now),
        energy_full: Some(full),
        power_now: Some(power),
        ..Default::default()
    }
}

#[test]
fn battery_estimate_only_with_a_real_rate_and_never_guessed() {
    // 30 Wh left at 15 W = 2 h.
    let s = battery::combine(
        &[bat("Discharging", 30_000_000, 60_000_000, 15_000_000)],
        Some(false),
    )
    .unwrap();
    assert_eq!((s.time_estimate_known, s.time_estimate_s), (true, 7200));
    assert_eq!(s.state, ChargeState::Discharging as i32);
    assert!(!s.on_ac && (s.percent - 50.0).abs() < 1e-9);
    // Charging: 30 Wh to go at 30 W = 1 h.
    let s = battery::combine(
        &[bat("Charging", 30_000_000, 60_000_000, 30_000_000)],
        Some(true),
    )
    .unwrap();
    assert_eq!(
        (s.time_estimate_known, s.time_estimate_s, s.on_ac),
        (true, 3600, true)
    );
    // On AC the battery reports power_now = 0 (ADR-0007): unknown, not zero seconds.
    let s = battery::combine(&[bat("Full", 60_000_000, 60_000_000, 0)], Some(true)).unwrap();
    assert_eq!((s.time_estimate_known, s.time_estimate_s), (false, 0));
    let s = battery::combine(&[bat("Discharging", 1, 2, 0)], None).unwrap();
    assert!(
        !s.time_estimate_known && !s.on_ac,
        "no rate: unknown; no mains file: derived from status"
    );
}

#[test]
fn several_batteries_sum_and_odd_inputs_are_survived() {
    let a = bat("Discharging", 10, 100, 5);
    let b = bat("Charging", 30, 100, 5);
    let s = battery::combine(&[a, b], Some(false)).unwrap();
    assert!((s.percent - 20.0).abs() < 1e-9, "40 of 200");
    assert_eq!(s.state, ChargeState::Charging as i32, "any charging wins");
    assert!(
        battery::combine(&[], Some(true)).is_none(),
        "desktop: no battery"
    );
    let only_cap = Reading {
        capacity: Some(77.0),
        status: Some("Weird".into()),
        ..Default::default()
    };
    let s = battery::combine(&[only_cap], None).unwrap();
    assert_eq!(
        (s.percent, s.state),
        (77.0, ChargeState::Unspecified as i32)
    );
    let nothing = Reading::default();
    assert!(
        battery::combine(&[nothing], None).is_none(),
        "no data at all: no sample"
    );
    assert_eq!(battery::parse_u64("  42\n"), Some(42));
    assert_eq!(battery::parse_u64("-1"), None);
    assert_eq!(
        battery::parse_status("Not charging"),
        ChargeState::NotCharging
    );
}

fn fake_battery(root: &Path, status: &str, cap: u64) {
    write(
        root,
        "sys/class/power_supply/BAT0/capacity",
        &format!("{cap}\n"),
    );
    write(
        root,
        "sys/class/power_supply/BAT0/status",
        &format!("{status}\n"),
    );
    write(root, "sys/class/power_supply/AC/online", "0\n");
}

#[tokio::test(start_paused = true)]
async fn battery_sensor_emits_on_change_and_at_least_every_interval() {
    let dir = tempfile::tempdir().unwrap();
    fake_battery(dir.path(), "Discharging", 80);
    let (sub, stop, task) = start(BatterySensor::with_root(&cfg(), dir.path()));
    let first = next(&sub, 6).await.expect("first reading");
    assert!(matches!(first.event, Some(Event::Battery(_))));
    assert!(
        next(&sub, 30).await.is_none(),
        "unchanged and well inside the interval: quiet"
    );
    fake_battery(dir.path(), "Discharging", 79);
    let changed = next(&sub, 10).await.expect("a change is emitted promptly");
    let Some(Event::Battery(b)) = changed.event else {
        panic!()
    };
    assert!((b.percent - 79.0).abs() < 1e-9);
    // Unchanged for the whole interval: re-emitted at 60 s.
    let again = next(&sub, 70).await.expect("forced re-emit");
    assert!(matches!(again.event, Some(Event::Battery(_))));
    stop.send(true).unwrap();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_machine_without_a_battery_is_healthy_and_silent() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("sys/class/power_supply/AC")).unwrap();
    let (sub, stop, task) = start(BatterySensor::with_root(&cfg(), dir.path()));
    assert!(next(&sub, 70).await.is_none());
    stop.send(true).unwrap();
    task.await.unwrap();
}

// ---------------------------------------------------------------- thermal (FR-MON-07)

fn hw(name: &str, temps: &[(&str, Option<&str>)]) -> Hwmon {
    Hwmon {
        name: name.into(),
        temps: temps
            .iter()
            .map(|(p, l)| ((*p).into(), l.map(str::to_owned)))
            .collect(),
    }
}

#[test]
fn the_package_sensor_is_chosen_by_driver_and_label() {
    let k10 = hw("k10temp", &[("t1", Some("Tctl")), ("t2", Some("Tccd1"))]);
    let nvme = hw("nvme", &[("n1", Some("Composite"))]);
    let picked = thermal::pick_sensor(&[nvme.clone(), k10]).unwrap();
    assert_eq!(
        (picked.0.as_str(), picked.1.to_str().unwrap()),
        ("k10temp", "t1")
    );
    let core = hw(
        "coretemp",
        &[("c0", Some("Core 0")), ("c1", Some("Package id 0"))],
    );
    assert_eq!(
        thermal::pick_sensor(&[core]).unwrap().1.to_str().unwrap(),
        "c1"
    );
    // k10temp present but no Tctl/Tdie label: do not pick a random core.
    assert!(thermal::pick_sensor(&[hw("k10temp", &[("x", Some("Tccd1"))])]).is_none());
    assert_eq!(
        thermal::pick_sensor(&[hw("acpitz", &[("a1", None)])])
            .unwrap()
            .0,
        "acpitz"
    );
    assert!(
        thermal::pick_sensor(&[nvme]).is_none(),
        "disks are not the CPU"
    );
    assert!(thermal::pick_sensor(&[]).is_none());
}

#[test]
fn millidegrees_convert_and_glitches_are_rejected() {
    assert_eq!(thermal::parse_millidegrees("45250\n"), Some(45.25));
    assert_eq!(thermal::parse_millidegrees("-5000"), Some(-5.0));
    for bad in ["", "hot", "999999", "-128000", "NaN", "1.5"] {
        assert_eq!(thermal::parse_millidegrees(bad), None, "{bad:?}");
    }
}

fn fake_k10(root: &Path, milli: &str) {
    write(root, "sys/class/hwmon/hwmon3/name", "k10temp\n");
    write(root, "sys/class/hwmon/hwmon3/temp1_label", "Tctl\n");
    write(root, "sys/class/hwmon/hwmon3/temp1_input", milli);
    write(root, "sys/class/hwmon/hwmon1/name", "nvme\n");
    write(root, "sys/class/hwmon/hwmon1/temp1_input", "30000\n");
}

#[tokio::test(start_paused = true)]
async fn thermal_sensor_reports_the_driver_name_and_rediscovers_after_a_bad_read() {
    let dir = tempfile::tempdir().unwrap();
    fake_k10(dir.path(), "61500\n");
    let (sub, stop, task) = start(ThermalSensor::with_root(&cfg(), dir.path()));
    let ev = next(&sub, 5).await.expect("first reading");
    let Some(Event::Thermal(t)) = ev.event else {
        panic!("thermal expected")
    };
    assert_eq!((t.sensor.as_str(), t.celsius), ("k10temp", 61.5));
    // Glitch, then recovery: the bad value is dropped, the sensor is found again.
    write(
        dir.path(),
        "sys/class/hwmon/hwmon3/temp1_input",
        "-128000\n",
    );
    assert!(next(&sub, 11).await.is_none());
    write(dir.path(), "sys/class/hwmon/hwmon3/temp1_input", "50000\n");
    let ev = next(&sub, 25).await.expect("recovers");
    assert!(matches!(ev.event, Some(Event::Thermal(t)) if t.celsius == 50.0));
    stop.send(true).unwrap();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn no_cpu_sensor_means_silence_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "sys/class/hwmon/hwmon0/name", "nvme\n");
    write(dir.path(), "sys/class/hwmon/hwmon0/temp1_input", "30000\n");
    let (sub, stop, task) = start(ThermalSensor::with_root(&cfg(), dir.path()));
    assert!(next(&sub, 35).await.is_none());
    stop.send(true).unwrap();
    task.await.unwrap();
}

// ---------------------------------------------------------------- never panic

proptest! {
    #[test]
    fn parsers_never_panic_on_arbitrary_text(text in ".{0,300}") {
        let _ = resources::parse_cpu_times(&text);
        let _ = resources::parse_meminfo(&text);
        let _ = disk::parse_mountinfo(&text);
        let _ = disk::relevant_mounts(&text);
        let _ = disk::decode_octal(&text);
        let _ = battery::parse_u64(&text);
        let _ = battery::parse_status(&text);
        let _ = thermal::parse_millidegrees(&text);
    }

    #[test]
    fn battery_combine_stays_in_range(
        now in any::<u64>(), full in any::<u64>(), power in any::<u64>(), cap in 0.0f64..200.0,
        status in prop_oneof![Just("Charging"), Just("Discharging"), Just("Full"), Just("x")],
    ) {
        let r = Reading {
            capacity: Some(cap), status: Some(status.into()),
            energy_now: Some(now), energy_full: Some(full), power_now: Some(power), ..Default::default()
        };
        if let Some(s) = battery::combine(&[r], None) {
            prop_assert!((0.0..=200.0).contains(&s.percent));
            prop_assert!(s.time_estimate_known || s.time_estimate_s == 0);
        }
    }
}

// ---------------------------------------------------------------- the real machine (local_)

#[tokio::test]
async fn local_real_machine_samples_are_sane() {
    use neuroos_monitor::sensors::resources::{collect_for_test, top_process};
    let (cpu_a, sample) = collect_for_test(Path::new("/proc")).expect("/proc readable");
    assert!(sample.mem_total_bytes > 0 && sample.mem_available_bytes <= sample.mem_total_bytes);
    assert!(top_process(Path::new("/proc"), rustix::param::page_size()).is_some());
    std::thread::sleep(Duration::from_millis(200));
    let (cpu_b, _) = collect_for_test(Path::new("/proc")).unwrap();
    if let Some(p) = resources::cpu_percent(cpu_a, cpu_b) {
        assert!((0.0..=100.0).contains(&p));
    }
    let text = std::fs::read_to_string("/proc/self/mountinfo").unwrap();
    for m in disk::relevant_mounts(&text) {
        if let Ok(s) = rustix::fs::statvfs(m.mount_point.as_str()) {
            assert!(s.f_blocks >= s.f_bfree);
        }
    }
    let (bats, mains) = battery::read_supplies(Path::new("/sys/class/power_supply"));
    if let Some(s) = battery::combine(&bats, mains) {
        assert!((0.0..=100.0).contains(&s.percent));
    }
    if let Some((_, path)) = thermal::pick_sensor(&thermal::discover(Path::new("/sys/class/hwmon")))
    {
        let c = thermal::parse_millidegrees(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!((-20.0..=130.0).contains(&c));
    }
}
