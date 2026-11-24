use anyhow::{Context, Result};
use cosmic_protocols::toplevel_info::v1::client::{
    zcosmic_toplevel_handle_v1, zcosmic_toplevel_info_v1,
};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

// -----------------------------------------------------------------------------
// Section 1: Disk (FR-MON-05)
// -----------------------------------------------------------------------------
fn probe_disk() -> serde_json::Value {
    let t0 = Instant::now();
    let mountinfo_path = "/proc/self/mountinfo";
    let content = match fs::read_to_string(mountinfo_path) {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "ok": false,
                "source": mountinfo_path,
                "latency_us": t0.elapsed().as_micros() as u64,
                "error": format!("Failed to read {}: {}", mountinfo_path, e),
                "filesystems": []
            });
        }
    };

    let pseudo_types = [
        "tmpfs",
        "devtmpfs",
        "proc",
        "sysfs",
        "devpts",
        "cgroup",
        "cgroup2",
        "pstore",
        "bpf",
        "autofs",
        "hugetlbfs",
        "mqueue",
        "debugfs",
        "tracefs",
        "fusectl",
        "configfs",
        "efivarfs",
        "binfmt_misc",
        "securityfs",
        "overlay",
        "ramfs",
        "nsfs",
    ];

    let mut filesystems = Vec::new();
    let mut seen_mounts = std::collections::HashSet::new();

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 7 {
            continue;
        }

        let root = parts[3];
        let mount_point = parts[4];

        // Find separator "-"
        let sep_idx = match parts.iter().position(|&p| p == "-") {
            Some(i) => i,
            None => continue,
        };

        if parts.len() <= sep_idx + 2 {
            continue;
        }

        let fstype = parts[sep_idx + 1];
        let mount_source = parts[sep_idx + 2];

        // Filter out pseudo, snap, loop, squashfs
        if pseudo_types.contains(&fstype)
            || fstype.starts_with("fuse.")
            || fstype == "squashfs"
            || mount_point.starts_with("/snap")
            || mount_source.starts_with("/dev/loop")
            || mount_point.starts_with("/dev/shm")
            || mount_point.starts_with("/setup/")
        {
            continue;
        }

        // Keep root of filesystem mounts (avoid subpath bind mounts when root is available)
        if root != "/" && !seen_mounts.is_empty() && seen_mounts.contains(mount_source) {
            continue;
        }

        // Call statvfs
        if let Ok(st) = rustix::fs::statvfs(mount_point) {
            let frsize = st.f_frsize;
            let total_bytes = st.f_blocks.saturating_mul(frsize);
            let free_bytes = st.f_bfree.saturating_mul(frsize);
            let available_bytes = st.f_bavail.saturating_mul(frsize);
            let used_bytes = total_bytes.saturating_sub(free_bytes);

            seen_mounts.insert(mount_source.to_string());
            filesystems.push(json!({
                "mount_point": mount_point,
                "fstype": fstype,
                "device": mount_source,
                "total_bytes": total_bytes,
                "used_bytes": used_bytes,
                "free_bytes": free_bytes,
                "available_bytes": available_bytes
            }));
        }
    }

    let ok = !filesystems.is_empty();
    json!({
        "ok": ok,
        "source": mountinfo_path,
        "latency_us": t0.elapsed().as_micros() as u64,
        "filesystems": filesystems
    })
}

// -----------------------------------------------------------------------------
// Section 2: Battery (FR-MON-06)
// -----------------------------------------------------------------------------
fn probe_battery() -> serde_json::Value {
    let t0 = Instant::now();
    let base_path = Path::new("/sys/class/power_supply");
    if !base_path.exists() {
        return json!({
            "ok": false,
            "source": "/sys/class/power_supply",
            "latency_us": t0.elapsed().as_micros() as u64,
            "error": "/sys/class/power_supply does not exist",
        });
    }

    // Check AC
    let mut on_ac = false;
    let mut ac_found = false;
    if let Ok(entries) = fs::read_dir(base_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("AC") || name.starts_with("ADP") {
                let online_path = entry.path().join("online");
                if let Ok(val) = fs::read_to_string(&online_path) {
                    ac_found = true;
                    if val.trim() == "1" {
                        on_ac = true;
                    }
                }
            }
        }
    }

    // Check Battery
    let mut bat_info = None;
    if let Ok(entries) = fs::read_dir(base_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("BAT") {
                let p = entry.path();
                let capacity = fs::read_to_string(p.join("capacity"))
                    .ok()
                    .and_then(|s| s.trim().parse::<u8>().ok());
                let status = fs::read_to_string(p.join("status"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|_| "Unknown".to_string());
                let energy_now = fs::read_to_string(p.join("energy_now"))
                    .ok()
                    .and_then(|s| s.trim().parse::<u64>().ok());
                let energy_full = fs::read_to_string(p.join("energy_full"))
                    .ok()
                    .and_then(|s| s.trim().parse::<u64>().ok());
                let power_now = fs::read_to_string(p.join("power_now"))
                    .ok()
                    .and_then(|s| s.trim().parse::<u64>().ok());

                let mut time_to_empty_s = None;
                let mut time_to_full_s = None;
                let estimate_status: String;

                let p_now = power_now.unwrap_or(0);
                if p_now == 0 {
                    estimate_status = format!("unavailable: power_now is 0 (status: {})", status);
                } else if status.eq_ignore_ascii_case("Discharging") {
                    if let Some(en) = energy_now {
                        let sec = (en.saturating_mul(3600)) / p_now;
                        time_to_empty_s = Some(sec);
                        estimate_status = format!("estimated discharging: {}s", sec);
                    } else {
                        estimate_status = "unavailable: energy_now unreadable".to_string();
                    }
                } else if status.eq_ignore_ascii_case("Charging") {
                    if let (Some(en), Some(ef)) = (energy_now, energy_full) {
                        let rem = ef.saturating_sub(en);
                        let sec = (rem.saturating_mul(3600)) / p_now;
                        time_to_full_s = Some(sec);
                        estimate_status = format!("estimated charging: {}s", sec);
                    } else {
                        estimate_status = "unavailable: energy values unreadable".to_string();
                    }
                } else {
                    estimate_status = format!("unavailable: status is {}", status);
                }

                bat_info = Some(json!({
                    "name": name,
                    "path": p.to_string_lossy(),
                    "capacity_pct": capacity,
                    "status": status,
                    "energy_now_uwh": energy_now,
                    "energy_full_uwh": energy_full,
                    "power_now_uw": power_now,
                    "time_to_empty_seconds": time_to_empty_s,
                    "time_to_full_seconds": time_to_full_s,
                    "estimate_status": estimate_status
                }));
                break;
            }
        }
    }

    let ok = ac_found || bat_info.is_some();
    json!({
        "ok": ok,
        "source": "/sys/class/power_supply",
        "latency_us": t0.elapsed().as_micros() as u64,
        "on_ac": on_ac,
        "battery": bat_info
    })
}

// -----------------------------------------------------------------------------
// Section 3: Thermal (FR-MON-07)
// -----------------------------------------------------------------------------
fn probe_thermal() -> serde_json::Value {
    let t0 = Instant::now();
    let mut hwmon_sensors = Vec::new();
    let mut thermal_zones = Vec::new();
    let mut cpu_package_sensor = None;
    let mut cpu_package_temp_celsius = None;

    // Scan /sys/class/hwmon/hwmon*
    let hwmon_base = Path::new("/sys/class/hwmon");
    if let Ok(entries) = fs::read_dir(hwmon_base) {
        let mut sorted_entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        sorted_entries.sort();

        for hdir in sorted_entries {
            let name = fs::read_to_string(hdir.join("name"))
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "unknown".to_string());

            if let Ok(inputs) = fs::read_dir(&hdir) {
                let mut temp_inputs: Vec<PathBuf> = inputs
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        let fname = p.file_name().unwrap_or_default().to_string_lossy();
                        fname.starts_with("temp") && fname.ends_with("_input")
                    })
                    .collect();
                temp_inputs.sort();

                for tpath in temp_inputs {
                    let fname = tpath.file_name().unwrap_or_default().to_string_lossy();
                    let prefix = fname.trim_end_matches("_input");
                    let label_path = hdir.join(format!("{}_label", prefix));
                    let label = fs::read_to_string(&label_path)
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default();

                    let val_opt = fs::read_to_string(&tpath)
                        .ok()
                        .and_then(|s| s.trim().parse::<i64>().ok());

                    if let Some(millicelsius) = val_opt {
                        let celsius = (millicelsius as f64) / 1000.0;
                        let is_cpu_package = (name == "k10temp" && label == "Tctl")
                            || (name == "coretemp" && label.starts_with("Package"));

                        if is_cpu_package {
                            cpu_package_sensor = Some(format!(
                                "{}/{} ({} {})",
                                hdir.file_name().unwrap_or_default().to_string_lossy(),
                                fname,
                                name,
                                label
                            ));
                            cpu_package_temp_celsius = Some(celsius);
                        }

                        hwmon_sensors.push(json!({
                            "path": tpath.to_string_lossy(),
                            "hwmon": hdir.file_name().unwrap_or_default().to_string_lossy(),
                            "name": name,
                            "label": label,
                            "temp_millicelsius": millicelsius,
                            "temp_celsius": celsius,
                            "is_cpu_package": is_cpu_package
                        }));
                    }
                }
            }
        }
    }

    // Scan /sys/class/thermal/thermal_zone*
    let thermal_base = Path::new("/sys/class/thermal");
    if let Ok(entries) = fs::read_dir(thermal_base) {
        let mut sorted_entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        sorted_entries.sort();

        for tdir in sorted_entries {
            let fname = tdir.file_name().unwrap_or_default().to_string_lossy();
            if fname.starts_with("thermal_zone") {
                let type_name = fs::read_to_string(tdir.join("type"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|_| "unknown".to_string());
                let val_opt = fs::read_to_string(tdir.join("temp"))
                    .ok()
                    .and_then(|s| s.trim().parse::<i64>().ok());

                if let Some(millicelsius) = val_opt {
                    let celsius = (millicelsius as f64) / 1000.0;
                    thermal_zones.push(json!({
                        "path": tdir.to_string_lossy(),
                        "zone": fname,
                        "type": type_name,
                        "temp_millicelsius": millicelsius,
                        "temp_celsius": celsius
                    }));
                }
            }
        }
    }

    let ok = !hwmon_sensors.is_empty() || !thermal_zones.is_empty();
    json!({
        "ok": ok,
        "source": "/sys/class/hwmon, /sys/class/thermal",
        "latency_us": t0.elapsed().as_micros() as u64,
        "cpu_package_sensor": cpu_package_sensor,
        "cpu_package_temp_celsius": cpu_package_temp_celsius,
        "hwmon_sensors": hwmon_sensors,
        "thermal_zones": thermal_zones
    })
}

// -----------------------------------------------------------------------------
// Section 4: Compositor (FR-MON-01, -02)
// -----------------------------------------------------------------------------
#[derive(Default)]
struct ToplevelState {
    has_app_id: bool,
    has_title: bool,
    states: Vec<String>,
}

struct WaylandAppData {
    globals: Vec<(u32, String, u32)>,
    toplevel_info: Option<zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1>,
    handles: Vec<zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1>,
    toplevel_states: HashMap<u32, ToplevelState>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for WaylandAppData {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            state.globals.push((name, interface.clone(), version));
            if interface == "zcosmic_toplevel_info_v1" && state.toplevel_info.is_none() {
                // Bind version 1 to receive Toplevel creation events directly
                let info = registry.bind::<zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1, _, _>(
                    name,
                    1,
                    qh,
                    (),
                );
                state.toplevel_info = Some(info);
            }
        }
    }
}

impl Dispatch<zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1, ()> for WaylandAppData {
    fn event(
        state: &mut Self,
        _: &zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1,
        event: zcosmic_toplevel_info_v1::Event,
        _: &(),
        _: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let zcosmic_toplevel_info_v1::Event::Toplevel { toplevel } = event {
            state.handles.push(toplevel);
        }
    }

    fn event_created_child(
        _opcode: u16,
        qh: &QueueHandle<Self>,
    ) -> Arc<dyn wayland_client::backend::ObjectData + 'static> {
        qh.make_data::<zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1, _>(())
    }
}

impl Dispatch<zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1, ()> for WaylandAppData {
    #[allow(clippy::chunks_exact_to_as_chunks)]
    fn event(
        state: &mut Self,
        proxy: &zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1,
        event: zcosmic_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = proxy.id().protocol_id();
        let entry = state.toplevel_states.entry(id).or_default();
        match event {
            // Only presence is recorded: app ids and titles are personal (R0-10, BUG-013).
            zcosmic_toplevel_handle_v1::Event::AppId { .. } => {
                entry.has_app_id = true;
            }
            zcosmic_toplevel_handle_v1::Event::Title { .. } => {
                entry.has_title = true;
            }
            zcosmic_toplevel_handle_v1::Event::State { state } => {
                for chunk in state.chunks_exact(4) {
                    let val = u32::from_ne_bytes(chunk.try_into().unwrap_or([0; 4]));
                    if let Ok(s) = zcosmic_toplevel_handle_v1::State::try_from(val) {
                        entry.states.push(format!("{:?}", s));
                    }
                }
            }
            _ => {}
        }
    }
}

fn probe_compositor() -> serde_json::Value {
    let t0 = Instant::now();
    let display_var = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "not set".to_string());
    let source_desc = format!("wayland: WAYLAND_DISPLAY={}", display_var);

    let conn = match Connection::connect_to_env() {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "ok": false,
                "source": source_desc,
                "latency_us": t0.elapsed().as_micros() as u64,
                "error": format!("Failed to connect to Wayland display: {}", e),
                "advertised_globals": [],
                "toplevels": [],
                "protocol_providing_fullscreen": null,
                "idle_notifier_offered": false,
                "session_lock_offered": false
            });
        }
    };

    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    let display = conn.display();
    let mut app_data = WaylandAppData {
        globals: Vec::new(),
        toplevel_info: None,
        handles: Vec::new(),
        toplevel_states: HashMap::new(),
    };

    let _registry = display.get_registry(&qh, ());
    if let Err(e) = event_queue.roundtrip(&mut app_data) {
        return json!({
            "ok": false,
            "source": source_desc,
            "latency_us": t0.elapsed().as_micros() as u64,
            "error": format!("Wayland registry roundtrip error: {}", e),
            "advertised_globals": [],
            "toplevels": [],
            "protocol_providing_fullscreen": null,
            "idle_notifier_offered": false,
            "session_lock_offered": false
        });
    }

    // Two more roundtrips to process bound globals and initial toplevel events
    let _ = event_queue.roundtrip(&mut app_data);
    let _ = event_queue.roundtrip(&mut app_data);

    let mut idle_notifier_offered = false;
    let mut session_lock_protocols = Vec::new();
    let mut globals_json = Vec::new();

    for (name, interface, version) in &app_data.globals {
        globals_json.push(json!({
            "name": name,
            "interface": interface,
            "version": version
        }));

        if interface == "ext_idle_notifier_v1" {
            idle_notifier_offered = true;
        }
        if interface.contains("session_lock") {
            session_lock_protocols.push(interface.clone());
        }
    }

    let session_lock_offered = !session_lock_protocols.is_empty();

    let mut toplevels_json = Vec::new();
    for (id, state) in &app_data.toplevel_states {
        let is_fullscreen = state.states.iter().any(|s| s == "Fullscreen");
        let is_activated = state.states.iter().any(|s| s == "Activated");
        let is_maximized = state.states.iter().any(|s| s == "Maximized");

        toplevels_json.push(json!({
            "id": id,
            "has_app_id": state.has_app_id,
            "has_title": state.has_title,
            "states": state.states,
            "is_fullscreen": is_fullscreen,
            "is_activated": is_activated,
            "is_maximized": is_maximized
        }));
    }

    let protocol_providing_fullscreen = if app_data.toplevel_info.is_some() {
        "zcosmic_toplevel_info_v1"
    } else {
        "none"
    };

    json!({
        "ok": true,
        "source": source_desc,
        "latency_us": t0.elapsed().as_micros() as u64,
        "advertised_globals_count": app_data.globals.len(),
        "advertised_globals": globals_json,
        "protocol_providing_fullscreen": protocol_providing_fullscreen,
        "idle_notifier_offered": idle_notifier_offered,
        "session_lock_offered": session_lock_offered,
        "session_lock_protocols": session_lock_protocols,
        "toplevels_count": toplevels_json.len(),
        "toplevels": toplevels_json
    })
}

// -----------------------------------------------------------------------------
// Section 5: Microphone In Use (FR-VOI-10)
// -----------------------------------------------------------------------------
fn probe_mic_in_use() -> serde_json::Value {
    let t0 = Instant::now();
    let source_desc = "pw-dump (PipeWire)";

    let output = match Command::new("pw-dump").output() {
        Ok(out) => out,
        Err(e) => {
            return json!({
                "ok": false,
                "source": source_desc,
                "latency_us": t0.elapsed().as_micros() as u64,
                "error": format!("Failed to execute pw-dump: {}", e),
                "mic_in_use": false,
                "capturing_nodes": []
            });
        }
    };

    if !output.status.success() {
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        return json!({
            "ok": false,
            "source": source_desc,
            "latency_us": t0.elapsed().as_micros() as u64,
            "error": format!("pw-dump failed with code {:?}: {}", output.status.code(), stderr_str.trim()),
            "mic_in_use": false,
            "capturing_nodes": []
        });
    }

    let parsed: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => {
            return json!({
                "ok": false,
                "source": source_desc,
                "latency_us": t0.elapsed().as_micros() as u64,
                "error": format!("Failed to parse pw-dump JSON output: {}", e),
                "mic_in_use": false,
                "capturing_nodes": []
            });
        }
    };

    let items = parsed.as_array();
    let mut capturing_nodes = Vec::new();
    let mut mic_in_use = false;

    if let Some(arr) = items {
        for item in arr {
            let info = item.get("info");
            let props = info.and_then(|i| i.get("props"));
            let media_class = props
                .and_then(|p| p.get("media.class"))
                .and_then(|m| m.as_str());

            if media_class == Some("Stream/Input/Audio") {
                let id = item.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
                let state = info
                    .and_then(|i| i.get("state"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("unknown");
                let app_name = props
                    .and_then(|p| p.get("application.name"))
                    .and_then(|a| a.as_str())
                    .map(|s| s.to_string());

                if state == "running" {
                    mic_in_use = true;
                }

                capturing_nodes.push(json!({
                    "id": id,
                    "application_name": app_name,
                    "state": state
                }));
            }
        }
    }

    json!({
        "ok": true,
        "source": source_desc,
        "latency_us": t0.elapsed().as_micros() as u64,
        "mic_in_use": mic_in_use,
        "capturing_nodes_count": capturing_nodes.len(),
        "capturing_nodes": capturing_nodes
    })
}

// -----------------------------------------------------------------------------
// Main: Consolidated Output
// -----------------------------------------------------------------------------
fn main() -> Result<()> {
    let disk = probe_disk();
    let battery = probe_battery();
    let thermal = probe_thermal();
    let compositor = probe_compositor();
    let mic_in_use = probe_mic_in_use();

    let output = json!({
        "probe": "octoplus-s05-sensors",
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp_utc": chrono_like_now(),
        "sections": {
            "disk": disk,
            "battery": battery,
            "thermal": thermal,
            "compositor": compositor,
            "microphone_in_use": mic_in_use
        }
    });

    println!(
        "{}",
        serde_json::to_string_pretty(&output).context("JSON serialize")?
    );
    Ok(())
}

fn chrono_like_now() -> String {
    let total_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let days = total_secs / 86400;
    let rem_secs = total_secs % 86400;
    let hours = rem_secs / 3600;
    let mins = (rem_secs % 3600) / 60;
    let secs = rem_secs % 60;

    let (year, month, day) = days_to_ymd(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, mins, secs
    )
}

fn days_to_ymd(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as i64, d as i64)
}
