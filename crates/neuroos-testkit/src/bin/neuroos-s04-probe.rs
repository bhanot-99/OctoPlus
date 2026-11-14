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

// Linux ABI constants for raw syscalls.
const AF_INET: i32 = 2;
const AF_INET6: i32 = 10;
const AF_PACKET: i32 = 17;
const SOCK_STREAM: i32 = 1;
const SOCK_RAW: i32 = 3;
const CLONE_NEWUSER: i32 = 0x10000000;
const PROT_READ: i32 = 1;
const PROT_WRITE: i32 = 2;
const PROT_EXEC: i32 = 4;
const MAP_PRIVATE: i32 = 0x02;
const MAP_ANONYMOUS: i32 = 0x20;
const PER_LINUX32: u64 = 0x0008;

unsafe extern "C" {
    fn socket(domain: i32, type_: i32, protocol: i32) -> i32;
    fn close(fd: i32) -> i32;
    fn unshare(flags: i32) -> i32;
    fn mmap(
        addr: *mut std::ffi::c_void,
        len: usize,
        prot: i32,
        flags: i32,
        fd: i32,
        offset: i64,
    ) -> *mut std::ffi::c_void;
    fn munmap(addr: *mut std::ffi::c_void, len: usize) -> i32;
    fn personality(persona: u64) -> i32;
    fn getuid() -> u32;
}

/// Helper to safely test raw socket creation.
fn raw_socket_test(domain: i32, sock_type: i32, proto: i32) -> std::io::Result<i32> {
    // SAFETY: socket() creates a socket descriptor with standard C library ABI.
    let fd = unsafe { socket(domain, sock_type, proto) };
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        // SAFETY: fd is valid and must be closed immediately.
        unsafe { close(fd) };
        Ok(fd)
    }
}

/// Helper to safely test unshare(CLONE_NEWUSER).
fn raw_unshare_user_ns() -> std::io::Result<()> {
    // SAFETY: unshare() takes standard bit flags.
    let rc = unsafe { unshare(CLONE_NEWUSER) };
    if rc < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Helper to safely test writable and executable memory allocation.
fn raw_mmap_wx() -> std::io::Result<()> {
    // SAFETY: mmap() requests a private anonymous page with PROT_WRITE | PROT_EXEC.
    let ptr = unsafe {
        mmap(
            std::ptr::null_mut(),
            4096,
            PROT_READ | PROT_WRITE | PROT_EXEC,
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    if ptr == (-1isize as *mut std::ffi::c_void) {
        Err(std::io::Error::last_os_error())
    } else {
        // SAFETY: ptr is non-null and allocated 4096 bytes.
        unsafe { munmap(ptr, 4096) };
        Ok(())
    }
}

/// Helper to safely test personality changes.
fn raw_personality_change() -> std::io::Result<()> {
    // SAFETY: personality() sets execution domain flags.
    let rc = unsafe { personality(PER_LINUX32) };
    if rc < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Helper to get current effective UID.
fn current_uid() -> u32 {
    // SAFETY: getuid() is always safe.
    unsafe { getuid() }
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
}

fn run_net(unix_dir: Option<&Path>) -> Result<()> {
    // 1. socket(AF_INET)
    match raw_socket_test(AF_INET, SOCK_STREAM, 0) {
        Ok(fd) => println!("FAIL socket(AF_INET) allowed (fd={fd})"),
        Err(e) => println!("PASS socket(AF_INET) blocked: {e}"),
    }

    // 2. socket(AF_INET6)
    match raw_socket_test(AF_INET6, SOCK_STREAM, 0) {
        Ok(fd) => println!("FAIL socket(AF_INET6) allowed (fd={fd})"),
        Err(e) => println!("PASS socket(AF_INET6) blocked: {e}"),
    }

    // 3. socket(AF_PACKET)
    match raw_socket_test(AF_PACKET, SOCK_RAW, 0) {
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
    let uid = current_uid();
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
    match raw_unshare_user_ns() {
        Ok(()) => println!("FAIL unshare(CLONE_NEWUSER) allowed"),
        Err(e) => println!("PASS unshare(CLONE_NEWUSER) blocked: {e}"),
    }

    // 3. mmap(PROT_WRITE | PROT_EXEC)
    match raw_mmap_wx() {
        Ok(()) => println!("FAIL mmap(PROT_WRITE|PROT_EXEC) allowed"),
        Err(e) => println!("PASS mmap(PROT_WRITE|PROT_EXEC) blocked: {e}"),
    }

    // 4. personality() change
    match raw_personality_change() {
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
