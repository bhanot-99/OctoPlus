//! Socket map as data (Architecture §5.2, Phase 1 P1-S01).
//!
//! The single source of truth for every IPC socket in Semantic-NeuroOS, its server,
//! location, and allowed clients.

use std::path::{Path, PathBuf};

use crate::paths::{FETCHER_RUNTIME_DIR, Part};

/// An allowed client for an IPC socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Client {
    /// A systemd service part.
    Part(Part),
    /// The user CLI tool (`neuroosctl`).
    Ctl,
}

impl Client {
    /// Returns the systemd unit name for a part client, or `None` for `Ctl`.
    #[must_use]
    pub fn unit_name(self) -> Option<String> {
        match self {
            Self::Part(p) => Some(p.unit()),
            Self::Ctl => None,
        }
    }
}

/// An entry in the socket map (Architecture §5.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketEntry {
    /// The part that listens on and serves this socket.
    pub server: Part,
    /// The socket name (e.g. "stream", "control", "health").
    pub socket: &'static str,
    /// Allowed clients that may connect to this socket.
    pub allowed_clients: &'static [Client],
}

impl SocketEntry {
    /// Returns the filesystem directory containing this socket.
    ///
    /// User parts: `<runtime_root>/<part>/<socket>`
    /// Fetcher: `/run/neuroos-fetcher/<socket>`
    #[must_use]
    pub fn socket_dir(&self, runtime_root: &Path) -> PathBuf {
        match self.server {
            Part::Fetcher => Path::new(FETCHER_RUNTIME_DIR).join(self.socket),
            _ => runtime_root.join(self.server.name()).join(self.socket),
        }
    }

    /// Returns the full socket file path (`<dir>/<socket>.sock`).
    #[must_use]
    pub fn socket_path(&self, runtime_root: &Path) -> PathBuf {
        self.socket_dir(runtime_root)
            .join(format!("{}.sock", self.socket))
    }

    /// Returns the list of systemd unit names allowed to connect to this socket.
    #[must_use]
    pub fn allowed_client_units(&self) -> Vec<String> {
        self.allowed_clients
            .iter()
            .filter_map(|c| c.unit_name())
            .collect()
    }

    /// Returns `true` if `neuroosctl` is allowed to connect to this socket.
    #[must_use]
    pub fn allows_ctl(&self) -> bool {
        self.allowed_clients
            .iter()
            .any(|c| matches!(c, Client::Ctl))
    }
}

/// Every IPC socket entry in Semantic-NeuroOS (Architecture §5.2).
pub const SOCKET_MAP: &[SocketEntry] = &[
    // Architecture §5.2 rows:
    // C1 monitor
    SocketEntry {
        server: Part::Monitor,
        socket: "stream",
        allowed_clients: &[Client::Part(Part::Storage)],
    },
    SocketEntry {
        server: Part::Monitor,
        socket: "control",
        allowed_clients: &[Client::Part(Part::Kernel), Client::Ctl],
    },
    // C2 voice
    SocketEntry {
        server: Part::Voice,
        socket: "voice",
        allowed_clients: &[Client::Part(Part::Knowledge)],
    },
    SocketEntry {
        server: Part::Voice,
        socket: "control",
        allowed_clients: &[Client::Part(Part::Kernel), Client::Ctl],
    },
    // C3 storage
    SocketEntry {
        server: Part::Storage,
        socket: "storage",
        allowed_clients: &[
            Client::Part(Part::Knowledge),
            Client::Part(Part::Kernel),
            Client::Ctl,
        ],
    },
    // C4 inference
    SocketEntry {
        server: Part::Inference,
        socket: "inference",
        allowed_clients: &[Client::Part(Part::Knowledge)],
    },
    // C5 knowledge
    SocketEntry {
        server: Part::Knowledge,
        socket: "knowledge",
        allowed_clients: &[Client::Part(Part::Voice), Client::Ctl],
    },
    // C6 kernel
    SocketEntry {
        server: Part::Kernel,
        socket: "kernel",
        allowed_clients: &[Client::Part(Part::Knowledge), Client::Ctl],
    },
    // D healthd
    SocketEntry {
        server: Part::Healthd,
        socket: "healthd",
        allowed_clients: &[Client::Ctl],
    },
    // C7 fetcher
    SocketEntry {
        server: Part::Fetcher,
        socket: "fetch",
        allowed_clients: &[Client::Part(Part::Kernel)],
    },
    // Health sockets (Architecture §5.2 row 10 & 12):
    SocketEntry {
        server: Part::Monitor,
        socket: "health",
        allowed_clients: &[Client::Part(Part::Healthd)],
    },
    SocketEntry {
        server: Part::Voice,
        socket: "health",
        allowed_clients: &[Client::Part(Part::Healthd)],
    },
    SocketEntry {
        server: Part::Storage,
        socket: "health",
        allowed_clients: &[Client::Part(Part::Healthd)],
    },
    SocketEntry {
        server: Part::Inference,
        socket: "health",
        allowed_clients: &[Client::Part(Part::Healthd)],
    },
    SocketEntry {
        server: Part::Knowledge,
        socket: "health",
        allowed_clients: &[Client::Part(Part::Healthd)],
    },
    SocketEntry {
        server: Part::Kernel,
        socket: "health",
        allowed_clients: &[Client::Part(Part::Healthd)],
    },
    SocketEntry {
        server: Part::Fetcher,
        socket: "health",
        allowed_clients: &[Client::Part(Part::Healthd)],
    },
];

/// Returns the socket path for an entry given the runtime root.
#[must_use]
pub fn socket_path_for_entry(entry: &SocketEntry, runtime_root: &Path) -> PathBuf {
    entry.socket_path(runtime_root)
}

/// Returns the allowed client unit names for a socket entry.
#[must_use]
pub fn allowed_client_units(entry: &SocketEntry) -> Vec<String> {
    entry.allowed_client_units()
}

/// Returns whether `neuroosctl` may connect to this socket entry.
#[must_use]
pub fn allows_ctl(entry: &SocketEntry) -> bool {
    entry.allows_ctl()
}

/// Returns the list of socket folders a given unit must see:
/// own folder + the folders of sockets it is an allowed client of.
///
/// Order is deterministic: own folder first, followed by client socket folders in
/// socket map order.
#[must_use]
pub fn visible_socket_folders(part: Part, runtime_root: &Path) -> Vec<PathBuf> {
    let mut folders = Vec::new();

    // 1. Own folder (Architecture §5.2, Decision 2).
    let own_dir = match part {
        Part::Fetcher => PathBuf::from(FETCHER_RUNTIME_DIR),
        _ => runtime_root.join(part.name()),
    };
    folders.push(own_dir);

    // 2. Folders of sockets where this part is an allowed client.
    for entry in SOCKET_MAP {
        if entry.server != part && entry.allowed_clients.contains(&Client::Part(part)) {
            let dir = entry.socket_dir(runtime_root);
            if !folders.contains(&dir) {
                folders.push(dir);
            }
        }
    }

    folders
}

/// Returns the visible socket folders for a part formatted with `%t/neuroos` as root,
/// excluding fetcher paths (which are under `/run/neuroos-fetcher`).
#[must_use]
pub fn user_runtime_visible_folders(part: Part) -> Vec<String> {
    let root = Path::new("%t/neuroos");
    visible_socket_folders(part, root)
        .into_iter()
        .filter_map(|p| {
            let s = p.to_string_lossy().into_owned();
            if s.starts_with("%t/neuroos") {
                Some(s)
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn mock_root() -> PathBuf {
        PathBuf::from("/run/user/1000/neuroos")
    }

    #[test]
    fn every_architecture_row_is_present() {
        // Architecture §5.2 specifies 12 rows:
        // monitor/stream, monitor/control, voice/voice, voice/control, storage/storage,
        // inference/inference, knowledge/knowledge, kernel/kernel, healthd/healthd,
        // <part>/health (6 user parts), fetcher/fetch, fetcher/health.
        let expected_sockets = [
            (Part::Monitor, "stream"),
            (Part::Monitor, "control"),
            (Part::Voice, "voice"),
            (Part::Voice, "control"),
            (Part::Storage, "storage"),
            (Part::Inference, "inference"),
            (Part::Knowledge, "knowledge"),
            (Part::Kernel, "kernel"),
            (Part::Healthd, "healthd"),
            (Part::Fetcher, "fetch"),
            (Part::Monitor, "health"),
            (Part::Voice, "health"),
            (Part::Storage, "health"),
            (Part::Inference, "health"),
            (Part::Knowledge, "health"),
            (Part::Kernel, "health"),
            (Part::Fetcher, "health"),
        ];

        assert_eq!(SOCKET_MAP.len(), expected_sockets.len());
        for (server, socket) in expected_sockets {
            let found = SOCKET_MAP
                .iter()
                .any(|e| e.server == server && e.socket == socket);
            assert!(found, "missing entry for {server:?}/{socket}");
        }
    }

    #[test]
    fn no_socket_folder_is_shared_by_two_sockets() {
        let root = mock_root();
        let mut dirs = HashSet::new();
        for entry in SOCKET_MAP {
            let dir = entry.socket_dir(&root);
            assert!(
                dirs.insert(dir.clone()),
                "duplicate socket folder found: {dir:?}"
            );
        }
    }

    #[test]
    fn visible_folders_never_include_unauthorized_socket() {
        let root = mock_root();
        for &part in &Part::ALL {
            let visible = visible_socket_folders(part, &root);
            for entry in SOCKET_MAP {
                let dir = entry.socket_dir(&root);
                let allowed =
                    (entry.server == part) || entry.allowed_clients.contains(&Client::Part(part));
                let sees = visible.iter().any(|v| dir.starts_with(v));
                if !allowed {
                    assert!(
                        !sees,
                        "part {part:?} must NOT see socket folder {dir:?} for entry {entry:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn property_part_sees_folder_iff_server_or_allowed_client() {
        let root = mock_root();
        for &part in &Part::ALL {
            let visible = visible_socket_folders(part, &root);
            for entry in SOCKET_MAP {
                let dir = entry.socket_dir(&root);
                let sees = visible.iter().any(|v| dir.starts_with(v));
                let expected =
                    (entry.server == part) || entry.allowed_clients.contains(&Client::Part(part));
                assert_eq!(
                    sees, expected,
                    "part {part:?} visibility mismatch for entry {entry:?}: sees={sees}, expected={expected}"
                );
            }
        }
    }

    #[test]
    fn allowed_client_units_and_ctl_check() {
        let stream = SOCKET_MAP
            .iter()
            .find(|e| e.server == Part::Monitor && e.socket == "stream")
            .unwrap();
        assert_eq!(
            allowed_client_units(stream),
            vec!["neuroos-storage.service".to_string()]
        );
        assert!(!allows_ctl(stream));

        let control = SOCKET_MAP
            .iter()
            .find(|e| e.server == Part::Monitor && e.socket == "control")
            .unwrap();
        assert_eq!(
            allowed_client_units(control),
            vec!["neuroos-kernel.service".to_string()]
        );
        assert!(allows_ctl(control));

        let healthd = SOCKET_MAP
            .iter()
            .find(|e| e.server == Part::Healthd && e.socket == "healthd")
            .unwrap();
        assert!(allowed_client_units(healthd).is_empty());
        assert!(allows_ctl(healthd));
    }
}
