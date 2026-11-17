//! Peer checks (AB-9, Architecture §5.2): every server checks the peer's UID
//! (`SO_PEERCRED`) and, where the policy names units, the peer's systemd unit.
//!
//! The unit is read from `/proc/<pid>/cgroup`. A PID can be reused between
//! `SO_PEERCRED` and the read; spike S-04 decides whether this check stays a
//! second layer behind mount-namespace socket isolation (Architecture §5.2) or
//! moves to pidfds. Security code fails closed (rules.md §6.5): anything we
//! cannot read is a rejection.

/// What we know about the process on the other end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    /// Peer UID from `SO_PEERCRED`.
    pub uid: u32,
    /// Peer PID from `SO_PEERCRED`, if the kernel reported one.
    pub pid: Option<i32>,
    /// systemd unit the peer runs in, if it could be read.
    pub unit: Option<String>,
}

/// Who may connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerPolicy {
    uids: Vec<u32>,
    units: Option<Vec<String>>,
    resolve_unit: bool,
}

impl PeerPolicy {
    /// Only processes of the server's own user, from any unit.
    #[must_use]
    pub fn same_user() -> Self {
        Self {
            uids: vec![current_uid()],
            units: None,
            resolve_unit: false,
        }
    }

    /// Only these UIDs, from any unit.
    #[must_use]
    pub fn uids(uids: Vec<u32>) -> Self {
        Self {
            uids,
            units: None,
            resolve_unit: false,
        }
    }

    /// Additionally require the peer to run in one of these systemd units
    /// (e.g. `neuroos-knowledge.service`).
    #[must_use]
    pub fn with_units(mut self, units: Vec<String>) -> Self {
        self.units = Some(units);
        self
    }

    /// Look up the peer's unit for every connection without restricting who may connect, so a
    /// handler can make per-message decisions on it (a socket that also admits `neuroosctl`
    /// has no unit list, yet C3 still needs to know whether a write came from C5 or C6).
    #[must_use]
    pub fn resolving_unit(mut self) -> Self {
        self.resolve_unit = true;
        self
    }

    /// Whether this policy needs the peer's unit.
    #[must_use]
    pub fn needs_unit(&self) -> bool {
        self.units.is_some() || self.resolve_unit
    }

    /// Decides whether `peer` may talk to us.
    ///
    /// # Errors
    /// A fixed reason string (never user content).
    pub fn check(&self, peer: &PeerInfo) -> Result<(), &'static str> {
        if !self.uids.contains(&peer.uid) {
            return Err("uid not allowed");
        }
        let Some(units) = &self.units else {
            return Ok(());
        };
        match &peer.unit {
            None => Err("peer unit unknown"),
            Some(u) if units.iter().any(|a| a == u) => Ok(()),
            Some(_) => Err("unit not allowed"),
        }
    }
}

/// UID of this process.
#[must_use]
pub fn current_uid() -> u32 {
    rustix::process::getuid().as_raw()
}

/// Extracts the innermost systemd unit (`*.service` or `*.scope`) from the
/// text of `/proc/<pid>/cgroup` (cgroup v2: a single `0::/path` line).
#[must_use]
pub fn unit_from_cgroup(text: &str) -> Option<String> {
    let line = text.lines().find(|l| l.starts_with("0::"))?;
    let path = line.strip_prefix("0::")?;
    path.split('/')
        .rev()
        .find(|seg| {
            [".service", ".scope"]
                .iter()
                .any(|sfx| seg.strip_suffix(sfx).is_some_and(|stem| !stem.is_empty()))
        })
        .map(str::to_owned)
}

/// Reads the systemd unit of `pid`.
#[must_use]
pub fn unit_of_pid(pid: i32) -> Option<String> {
    if pid <= 0 {
        return None;
    }
    let text = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    unit_from_cgroup(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(uid: u32, unit: Option<&str>) -> PeerInfo {
        PeerInfo {
            uid,
            pid: Some(42),
            unit: unit.map(str::to_owned),
        }
    }

    #[test]
    fn resolving_the_unit_does_not_restrict_anyone() {
        let p = PeerPolicy::uids(vec![1000]);
        assert!(!p.needs_unit());
        let p = p.resolving_unit();
        assert!(p.needs_unit());
        // Any unit, or none, still passes the connection check; only the uid matters.
        assert!(p.check(&peer(1000, None)).is_ok());
        assert!(p.check(&peer(1000, Some("anything.service"))).is_ok());
        assert_eq!(
            p.check(&peer(1001, Some("x.service"))),
            Err("uid not allowed")
        );
    }

    #[test]
    fn policy_decision_table() {
        let any_unit = PeerPolicy::uids(vec![1000]);
        let units =
            PeerPolicy::uids(vec![1000]).with_units(vec!["neuroos-knowledge.service".into()]);
        let cases = [
            (&any_unit, peer(1000, None), Ok(())),
            (&any_unit, peer(1000, Some("x.service")), Ok(())),
            (&any_unit, peer(0, None), Err("uid not allowed")),
            (
                &units,
                peer(1000, Some("neuroos-knowledge.service")),
                Ok(()),
            ),
            (
                &units,
                peer(1000, Some("neuroos-voice.service")),
                Err("unit not allowed"),
            ),
            (&units, peer(1000, None), Err("peer unit unknown")),
            (
                &units,
                peer(1001, Some("neuroos-knowledge.service")),
                Err("uid not allowed"),
            ),
        ];
        for (policy, p, want) in cases {
            assert_eq!(policy.check(&p), want, "{p:?}");
        }
        assert!(units.needs_unit());
        assert!(!any_unit.needs_unit());
    }

    #[test]
    fn empty_uid_list_rejects_everyone() {
        assert_eq!(
            PeerPolicy::uids(vec![]).check(&peer(1000, None)),
            Err("uid not allowed")
        );
    }

    #[test]
    fn same_user_accepts_self() {
        assert_eq!(
            PeerPolicy::same_user().check(&peer(current_uid(), None)),
            Ok(())
        );
    }

    #[test]
    fn cgroup_parsing() {
        let cases = [
            (
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/neuroos-storage.service\n",
                Some("neuroos-storage.service"),
            ),
            (
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/run-u12.scope\n",
                Some("run-u12.scope"),
            ),
            (
                "0::/user.slice/user-1000.slice/session-2.scope\n",
                Some("session-2.scope"),
            ),
            ("0::/\n", None),
            ("", None),
            ("1:name=systemd:/x.service\n", None),
            ("0::/a/.service\n", None),
        ];
        for (text, want) in cases {
            assert_eq!(unit_from_cgroup(text).as_deref(), want, "{text:?}");
        }
    }

    #[test]
    fn own_unit_is_readable_and_bad_pids_are_none() {
        let me = i32::try_from(std::process::id()).unwrap();
        // Under a test runner we are in some scope or service; the read must work.
        assert!(unit_of_pid(me).is_some());
        assert_eq!(unit_of_pid(0), None);
        assert_eq!(unit_of_pid(-1), None);
        assert_eq!(unit_of_pid(i32::MAX), None);
    }
}
