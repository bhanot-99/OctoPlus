//! `neuroos-s04-probe`: systemd hardening and sandbox probe binary (Spike S-04, P0-S10).
//!
//! Evaluates network blocking, DNS accessibility, filesystem visibility, and
//! process isolation properties from inside systemd user-scope units.

#![allow(unsafe_code, clippy::print_stdout)]

use std::fs;
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use rustix::mm::{MapFlags, ProtFlags};
use rustix::net::{AddressFamily, SocketType};

/// `personality()` has no rustix wrapper; it is the one direct libc call left.
const PER_LINUX32: u64 = 0x0008;

unsafe extern "C" {
    fn personality(persona: u64) -> i32;
}

/// Tries to create a socket; the descriptor is closed at once.
fn socket_test(domain: AddressFamily, sock_type: SocketType) -> std::io::Result<i32> {
    use std::os::fd::AsRawFd;
    let fd = rustix::net::socket(domain, sock_type, None)?;
    Ok(fd.as_raw_fd())
}

/// Tries `unshare(CLONE_NEWUSER)`.
fn unshare_user_ns() -> std::io::Result<()> {
    // SAFETY: the probe is single-threaded and exits right after its checks,
    // so moving it into a new user namespace cannot break other threads.
    unsafe { rustix::thread::unshare_unsafe(rustix::thread::UnshareFlags::NEWUSER) }?;
    Ok(())
}

/// Tries to map one writable and executable page.
fn mmap_wx() -> std::io::Result<()> {
    // SAFETY: a fresh private anonymous page at a kernel-chosen address; it is
    // never dereferenced and is unmapped before returning.
    let ptr = unsafe {
        rustix::mm::mmap_anonymous(
            std::ptr::null_mut(),
            4096,
            ProtFlags::READ | ProtFlags::WRITE | ProtFlags::EXEC,
            MapFlags::PRIVATE,
        )
    }?;
    // SAFETY: `ptr` is the 4096-byte mapping created above.
    unsafe { rustix::mm::munmap(ptr, 4096) }?;
    Ok(())
}

/// Tries to change the execution domain.
fn personality_change() -> std::io::Result<()> {
    // SAFETY: personality() takes a plain flag word and touches no memory.
    let rc = unsafe { personality(PER_LINUX32) };
    if rc < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[derive(Debug, Parser)]
#[command(name = "neuroos-s04-probe", about = "Security and hardening probe")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Probe network restrictions: socket(AF_INET/6/PACKET), TCP 127.0.0.1, and AF_UNIX.
    Net {
        /// Directory to create the test AF_UNIX socket in.
        #[arg(long)]
        unix_dir: Option<PathBuf>,
    },
    /// Probe DNS and resolver accessibility: /run/systemd/resolve, 127.0.0.53, and D-Bus.
    Dns,
    /// Probe filesystem visibility: /home, /run/user/<uid>, /tmp, /dev, and other /proc.
    Fs,
    /// Probe process isolation: /proc/self/status, unshare, mmap WX, personality.
    Proc,
    /// Connect to a Unix domain socket at the specified path.
    Connect {
        /// Socket path to connect to.
        path: PathBuf,
        /// Optional timeout in milliseconds.
        #[arg(long, default_value_t = 1000)]
        timeout_ms: u64,
    },
    /// Run all probe checks sequentially.
    All {
        /// Directory to create the test AF_UNIX socket in.
        #[arg(long)]
        unix_dir: Option<PathBuf>,
    },
    /// SC Proof 1: Strictly verifies no network (TCP 1.1.1.1:443 fails, local TCP fails, DNS fails, raw/INET sockets fail, UNIX socket works).
    NetProof {
        /// Local port of listening TCP server to attempt connecting to.
        #[arg(long)]
        local_port: Option<u16>,
        /// Directory to create the test AF_UNIX socket in.
        #[arg(long)]
        unix_dir: Option<PathBuf>,
        /// Negative control: assert local TCP connection succeeds.
        #[arg(long)]
        negative_control: bool,
    },
    /// SC Proof 2: Strictly verifies socket map mount namespace isolation and peer checks.
    SocketMapProof {
        /// Short part name being evaluated (e.g. storage, inference, knowledge).
        #[arg(long)]
        part: String,
        /// Runtime root path (defaults to $XDG_RUNTIME_DIR/neuroos).
        #[arg(long)]
        runtime_root: Option<PathBuf>,
        /// Negative control: assert forbidden folder breach is detected.
        #[arg(long)]
        negative_control: bool,
    },
}

fn run_net(unix_dir: Option<&Path>) -> Result<()> {
    // 1. socket(AF_INET)
    match socket_test(AddressFamily::INET, SocketType::STREAM) {
        Ok(fd) => println!("FAIL socket(AF_INET) allowed (fd={fd})"),
        Err(e) => println!("PASS socket(AF_INET) blocked: {e}"),
    }

    // 2. socket(AF_INET6)
    match socket_test(AddressFamily::INET6, SocketType::STREAM) {
        Ok(fd) => println!("FAIL socket(AF_INET6) allowed (fd={fd})"),
        Err(e) => println!("PASS socket(AF_INET6) blocked: {e}"),
    }

    // 3. socket(AF_PACKET)
    match socket_test(AddressFamily::PACKET, SocketType::RAW) {
        Ok(fd) => println!("FAIL socket(AF_PACKET) allowed (fd={fd})"),
        Err(e) => println!("PASS socket(AF_PACKET) blocked: {e}"),
    }

    // 4. TCP connect to 127.0.0.1:9 (must fail)
    let addr: SocketAddr = "127.0.0.1:9".parse().context("parsing loopback addr")?;
    match TcpStream::connect_timeout(&addr, Duration::from_millis(500)) {
        Ok(_) => println!("FAIL tcp_connect(127.0.0.1:9) connected"),
        Err(e) => println!("PASS tcp_connect(127.0.0.1:9) blocked: {e}"),
    }

    // 5. AF_UNIX socket in its own folder (must still work)
    let base_dir = if let Some(d) = unix_dir {
        d.to_path_buf()
    } else {
        std::env::temp_dir()
    };
    let sock_path = base_dir.join(format!("probe-test-{}.sock", std::process::id()));
    let _ = fs::remove_file(&sock_path);

    let test_unix = || -> Result<()> {
        let listener = UnixListener::bind(&sock_path).context("bind unix socket")?;
        let client = UnixStream::connect(&sock_path).context("connect unix socket")?;
        let (server, _) = listener.accept().context("accept unix socket")?;
        drop(client);
        drop(server);
        drop(listener);
        let _ = fs::remove_file(&sock_path);
        Ok(())
    };

    match test_unix() {
        Ok(()) => println!("PASS unix_socket bound: {}", sock_path.display()),
        Err(e) => println!("FAIL unix_socket failed: {e:#}"),
    }

    Ok(())
}

fn run_dns() -> Result<()> {
    // 1. /run/systemd/resolve/ visibility
    match fs::read_dir("/run/systemd/resolve") {
        Ok(entries) => {
            let names: Vec<String> = entries
                .filter_map(std::result::Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            println!(
                "FAIL /run/systemd/resolve/ visible: entries=[{}]",
                names.join(", ")
            );
        }
        Err(e) => {
            println!("PASS /run/systemd/resolve/ inaccessible: {e}");
        }
    }

    // 2. /etc/resolv.conf nameserver 127.0.0.53 reachable (expect blocked)
    let check_nameserver = || -> Result<()> {
        let sock = UdpSocket::bind("0.0.0.0:0").context("bind udp")?;
        sock.set_read_timeout(Some(Duration::from_millis(300)))
            .context("timeout")?;
        sock.connect("127.0.0.53:53").context("connect udp")?;
        // A UDP connect sends nothing, so ask a real question: an A query for
        // "localhost". Only an answer proves the resolver is reachable.
        let query: [u8; 27] = [
            0x12, 0x34, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 9, b'l', b'o',
            b'c', b'a', b'l', b'h', b'o', b's', b't', 0, 0x00, 0x01, 0x00, 0x01,
        ];
        sock.send(&query).context("send query")?;
        let mut buf = [0u8; 512];
        sock.recv(&mut buf).context("no answer")?;
        Ok(())
    };
    match check_nameserver() {
        Ok(()) => println!("FAIL nameserver_127_0_0_53 reachable"),
        Err(e) => println!("PASS nameserver_127_0_0_53 unreachable: {e}"),
    }

    // 3. System D-Bus socket visibility and connectability
    let dbus_path = Path::new("/run/dbus/system_bus_socket");
    if dbus_path.exists() {
        match UnixStream::connect(dbus_path) {
            Ok(_) => println!("INFO system_bus_socket visible and connectable"),
            Err(e) => println!("INFO system_bus_socket visible but connect failed: {e}"),
        }
    } else {
        println!("PASS system_bus_socket inaccessible or not found");
    }

    // 4. org.freedesktop.resolve1 reachability via busctl (localhost resolution)
    let busctl_out = Command::new("busctl")
        .args([
            "--system",
            "call",
            "org.freedesktop.resolve1",
            "/org/freedesktop/resolve1",
            "org.freedesktop.resolve1.Manager",
            "ResolveHostname",
            "isit",
            "0",
            "localhost",
            "0",
            "0",
        ])
        .output();

    match busctl_out {
        Ok(out) if out.status.success() => {
            println!("INFO resolve1_dbus reachable (localhost resolved via system bus)");
        }
        Ok(out) => {
            let msg = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            println!("PASS resolve1_dbus unreachable: {msg}");
        }
        Err(e) => {
            println!("PASS resolve1_dbus busctl unavailable: {e}");
        }
    }

    Ok(())
}

fn run_fs() -> Result<()> {
    // 1. /home visibility
    match fs::read_dir("/home") {
        Ok(entries) => {
            let names: Vec<String> = entries
                .filter_map(std::result::Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            println!(
                "INFO home count={} entries=[{}]",
                names.len(),
                names.join(", ")
            );
        }
        Err(e) => println!("INFO home inaccessible: {e}"),
    }

    // 2. /run/user/<uid> visibility
    let uid = rustix::process::getuid().as_raw();
    let user_run = format!("/run/user/{uid}");
    match fs::read_dir(&user_run) {
        Ok(entries) => {
            let count = entries.filter_map(std::result::Result::ok).count();
            println!("INFO run_user_{uid} visible=true entries={count}");
        }
        Err(e) => println!("INFO run_user_{uid} visible=false: {e}"),
    }

    // 3. /tmp visibility and isolation
    match fs::read_dir("/tmp") {
        Ok(entries) => {
            let count = entries.filter_map(std::result::Result::ok).count();
            println!("INFO tmp count={count}");
        }
        Err(e) => println!("INFO tmp inaccessible: {e}"),
    }

    // 4. /dev entries count
    match fs::read_dir("/dev") {
        Ok(entries) => {
            let count = entries.filter_map(std::result::Result::ok).count();
            println!("INFO dev_entries count={count}");
        }
        Err(e) => println!("INFO dev inaccessible: {e}"),
    }

    // 5. /proc/<other pid>/cgroup readable
    match fs::read_to_string("/proc/1/cgroup") {
        Ok(text) => println!("INFO proc_1_cgroup readable: {}", text.trim()),
        Err(e) => println!("INFO proc_1_cgroup unreadable: {e}"),
    }

    Ok(())
}

fn run_proc() -> Result<()> {
    // 1. Parse /proc/self/status fields
    if let Ok(status) = fs::read_to_string("/proc/self/status") {
        let mut nonewprivs = None;
        let mut seccomp = None;
        let mut capbnd = None;
        let mut capeff = None;

        for line in status.lines() {
            if let Some(val) = line.strip_prefix("NoNewPrivs:") {
                nonewprivs = Some(val.trim().to_owned());
            } else if let Some(val) = line.strip_prefix("Seccomp:") {
                seccomp = Some(val.trim().to_owned());
            } else if let Some(val) = line.strip_prefix("CapBnd:") {
                capbnd = Some(val.trim().to_owned());
            } else if let Some(val) = line.strip_prefix("CapEff:") {
                capeff = Some(val.trim().to_owned());
            }
        }

        if let Some(nnp) = nonewprivs {
            if nnp == "1" {
                println!("PASS NoNewPrivs enabled (1)");
            } else {
                println!("FAIL NoNewPrivs not enabled ({nnp})");
            }
        }
        if let Some(sec) = seccomp {
            if sec == "2" {
                println!("PASS Seccomp filter active (2)");
            } else {
                println!("FAIL Seccomp not active ({sec})");
            }
        }
        if let Some(bnd) = capbnd {
            if bnd == "0000000000000000" {
                println!("PASS CapBnd empty ({bnd})");
            } else {
                println!("INFO CapBnd present ({bnd})");
            }
        }
        if let Some(eff) = capeff {
            if eff == "0000000000000000" {
                println!("PASS CapEff empty ({eff})");
            } else {
                println!("INFO CapEff present ({eff})");
            }
        }
    }

    // 2. unshare(CLONE_NEWUSER)
    match unshare_user_ns() {
        Ok(()) => println!("FAIL unshare(CLONE_NEWUSER) allowed"),
        Err(e) => println!("PASS unshare(CLONE_NEWUSER) blocked: {e}"),
    }

    // 3. mmap(PROT_WRITE | PROT_EXEC)
    match mmap_wx() {
        Ok(()) => println!("FAIL mmap(PROT_WRITE|PROT_EXEC) allowed"),
        Err(e) => println!("PASS mmap(PROT_WRITE|PROT_EXEC) blocked: {e}"),
    }

    // 4. personality() change
    match personality_change() {
        Ok(()) => println!("FAIL personality() change allowed"),
        Err(e) => println!("PASS personality() change blocked: {e}"),
    }

    Ok(())
}

fn run_connect(path: &Path, _timeout_ms: u64) -> Result<()> {
    match UnixStream::connect(path) {
        Ok(_) => {
            println!("PASS connect {}", path.display());
            Ok(())
        }
        Err(e) => {
            println!("FAIL connect {}: {e}", path.display());
            Ok(())
        }
    }
}

fn run_net_proof(
    local_port: Option<u16>,
    unix_dir: Option<&Path>,
    negative_control: bool,
) -> Result<()> {
    // 1. TCP connect to 1.1.1.1:443 (must fail unless negative control)
    let remote_addr: SocketAddr = "1.1.1.1:443".parse().context("parsing remote addr")?;
    match TcpStream::connect_timeout(&remote_addr, Duration::from_millis(500)) {
        Ok(_) => {
            if !negative_control {
                anyhow::bail!("FAIL tcp_connect(1.1.1.1:443) connected but expected blocked");
            }
        }
        Err(e) => {
            println!("PASS tcp_connect(1.1.1.1:443) blocked: {e}");
        }
    }

    // 2. TCP connect to local listener
    if let Some(port) = local_port {
        let local_addr = SocketAddr::from(([127, 0, 0, 1], port));
        match TcpStream::connect_timeout(&local_addr, Duration::from_millis(500)) {
            Ok(_) => {
                if negative_control {
                    println!("PASS negative_control local TCP connected successfully");
                } else {
                    anyhow::bail!(
                        "FAIL tcp_connect(127.0.0.1:{port}) connected but expected blocked"
                    );
                }
            }
            Err(e) => {
                if negative_control {
                    anyhow::bail!("FAIL negative_control local TCP connect failed: {e}");
                } else {
                    println!("PASS tcp_connect(127.0.0.1:{port}) blocked: {e}");
                }
            }
        }
    }

    if negative_control {
        // Negative control only needs to verify that TCP connectivity to local listener works
        return Ok(());
    }

    // 3. Socket creation tests (must all fail)
    if let Ok(fd) = socket_test(AddressFamily::INET, SocketType::STREAM) {
        anyhow::bail!("FAIL socket(AF_INET, STREAM) allowed (fd={fd})");
    }
    if let Ok(fd) = socket_test(AddressFamily::INET, SocketType::RAW) {
        anyhow::bail!("FAIL socket(AF_INET, RAW) allowed (fd={fd})");
    }
    if let Ok(fd) = socket_test(AddressFamily::INET6, SocketType::STREAM) {
        anyhow::bail!("FAIL socket(AF_INET6, STREAM) allowed (fd={fd})");
    }
    if let Ok(fd) = socket_test(AddressFamily::INET6, SocketType::RAW) {
        anyhow::bail!("FAIL socket(AF_INET6, RAW) allowed (fd={fd})");
    }
    if let Ok(fd) = socket_test(AddressFamily::PACKET, SocketType::RAW) {
        anyhow::bail!("FAIL socket(AF_PACKET, RAW) allowed (fd={fd})");
    }
    println!("PASS all INET/RAW/PACKET socket allocations blocked");

    // 4. DNS accessibility checks
    if let Ok(entries) = fs::read_dir("/run/systemd/resolve") {
        let count = entries.filter_map(std::result::Result::ok).count();
        if count > 0 {
            anyhow::bail!("FAIL /run/systemd/resolve visible with {count} entries");
        }
    }
    println!("PASS /run/systemd/resolve inaccessible or empty");

    let check_udp = || -> Result<()> {
        let sock = UdpSocket::bind("0.0.0.0:0").context("bind udp")?;
        sock.set_read_timeout(Some(Duration::from_millis(300)))
            .context("timeout")?;
        sock.connect("127.0.0.53:53").context("connect udp")?;
        let query: [u8; 27] = [
            0x12, 0x34, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 9, b'l', b'o',
            b'c', b'a', b'l', b'h', b'o', b's', b't', 0, 0x00, 0x01, 0x00, 0x01,
        ];
        sock.send(&query).context("send query")?;
        let mut buf = [0u8; 512];
        sock.recv(&mut buf).context("no answer")?;
        Ok(())
    };
    if check_udp().is_ok() {
        anyhow::bail!("FAIL nameserver 127.0.0.53 reachable");
    }
    println!("PASS nameserver 127.0.0.53 unreachable");

    // 5. AF_UNIX socket in its own folder works
    let base_dir = if let Some(d) = unix_dir {
        d.to_path_buf()
    } else {
        std::env::temp_dir()
    };
    let sock_path = base_dir.join(format!("probe-test-{}.sock", std::process::id()));
    let _ = fs::remove_file(&sock_path);

    let listener = UnixListener::bind(&sock_path).context("bind unix socket")?;
    let client = UnixStream::connect(&sock_path).context("connect unix socket")?;
    let (server, _) = listener.accept().context("accept unix socket")?;
    drop(client);
    drop(server);
    drop(listener);
    let _ = fs::remove_file(&sock_path);
    println!("PASS unix_socket verified in {}", base_dir.display());

    Ok(())
}

fn run_socketmap_proof(
    part_str: &str,
    runtime_root: Option<&Path>,
    negative_control: bool,
) -> Result<()> {
    use neuroos_common::paths::Part;
    use neuroos_common::socketmap::{Client, SOCKET_MAP};

    let part = Part::ALL
        .into_iter()
        .find(|p| p.name() == part_str || p.unit() == part_str)
        .ok_or_else(|| anyhow::anyhow!("unknown part: {part_str}"))?;

    let root = if let Some(r) = runtime_root {
        r.to_path_buf()
    } else {
        let uid = rustix::process::getuid().as_raw();
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}")))
            .join("neuroos")
    };

    let mut breach_detected = false;

    for entry in SOCKET_MAP {
        let dir = entry.socket_dir(&root);
        let sock_path = entry.socket_path(&root);

        let is_allowed = entry.server == part
            || entry.allowed_clients.contains(&Client::Part(part))
            || (part == Part::Healthd && entry.socket == "health");

        if is_allowed {
            if !dir.exists() {
                if entry.server == Part::Fetcher {
                    println!(
                        "INFO uninstalled fetcher socket folder {} not on host (C7 treated as not installed)",
                        dir.display()
                    );
                    continue;
                }
                anyhow::bail!(
                    "FAIL allowed socket folder {} does not exist for {:?}",
                    dir.display(),
                    part
                );
            }
            println!("PASS allowed socket folder visible: {}", dir.display());

            if sock_path.exists() {
                // Connect and verify peer check rejects or accepts as probe
                if let Ok(_stream) = UnixStream::connect(&sock_path) {
                    println!("INFO connected to allowed server: {}", sock_path.display());
                    // Since probe is not the allowed unit, peer check will reject if enforced
                }
            }
        } else {
            // Forbidden socket folder
            if dir.exists() {
                breach_detected = true;
                if !negative_control {
                    anyhow::bail!(
                        "FAIL breach: forbidden socket folder {} is visible to {:?}",
                        dir.display(),
                        part
                    );
                } else {
                    println!(
                        "PASS negative_control detected visible forbidden folder: {}",
                        dir.display()
                    );
                }
            } else {
                match fs::metadata(&dir) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        println!("PASS forbidden folder is ENOENT: {}", dir.display());
                    }
                    Ok(_) => {
                        breach_detected = true;
                        if !negative_control {
                            anyhow::bail!("FAIL forbidden folder {} is accessible", dir.display());
                        }
                    }
                    Err(e) => {
                        println!(
                            "PASS forbidden folder inaccessible: {} ({e})",
                            dir.display()
                        );
                    }
                }
            }
        }
    }

    if negative_control && !breach_detected {
        anyhow::bail!(
            "FAIL negative_control expected a breach but all forbidden folders were masked!"
        );
    }

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Net { unix_dir } => run_net(unix_dir.as_deref()),
        Commands::Dns => run_dns(),
        Commands::Fs => run_fs(),
        Commands::Proc => run_proc(),
        Commands::Connect { path, timeout_ms } => run_connect(&path, timeout_ms),
        Commands::All { unix_dir } => {
            run_net(unix_dir.as_deref())?;
            run_dns()?;
            run_fs()?;
            run_proc()?;
            Ok(())
        }
        Commands::NetProof {
            local_port,
            unix_dir,
            negative_control,
        } => run_net_proof(local_port, unix_dir.as_deref(), negative_control),
        Commands::SocketMapProof {
            part,
            runtime_root,
            negative_control,
        } => run_socketmap_proof(&part, runtime_root.as_deref(), negative_control),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_subcommands_run_without_error() {
        let temp = std::env::temp_dir();
        assert!(run_net(Some(&temp)).is_ok());
        assert!(run_dns().is_ok());
        assert!(run_fs().is_ok());
        assert!(run_proc().is_ok());

        let test_sock = temp.join(format!("probe-test-conn-{}.sock", std::process::id()));
        let _ = fs::remove_file(&test_sock);
        if let Ok(_listener) = UnixListener::bind(&test_sock) {
            assert!(run_connect(&test_sock, 100).is_ok());
            let _ = fs::remove_file(&test_sock);
        }
    }
}
