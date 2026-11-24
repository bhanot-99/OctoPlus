//! The private stdin/stdout contract between C6 and its dialog child
//! (Architecture §8.4 steps 2–3, security.md §4.5).
//!
//! C6 writes one [`DialogRequest`] as JSON to the dialog's stdin and closes it.
//! The dialog writes exactly one [`DialogDecision`] line to stdout and exits.
//! Anything else — EOF without a line, a malformed line, the wrong action id,
//! an edited query on a form that is not editable — is treated by C6 as
//! "no approval" (fail closed, rules.md §6.5).
//!
//! Every string here is shown on screen, so validation rejects control
//! characters and caps lengths: the dialog renders data, never markup.

use serde::{Deserialize, Serialize};

/// Contract version; both sides must agree exactly.
pub const PROTOCOL_VERSION: u32 = 1;
/// Largest request C6 may send (the full cleanup list fits easily).
pub const MAX_REQUEST_BYTES: usize = 256 * 1024;
/// Largest decision line the dialog may return.
pub const MAX_DECISION_BYTES: usize = 4 * 1024;
/// Longest approval window (Architecture §8.4: tokens live 120 s at most).
pub const MAX_EXPIRY_MS: u64 = 120_000;
/// Upper bound on the approve-button delay a request may ask for.
pub const MAX_APPROVE_DELAY_MS: u64 = 5_000;
/// Most items one dialog lists; C6 refuses larger plans before asking.
pub const MAX_ITEMS: usize = 64;
/// Most parameter rows one dialog shows.
pub const MAX_PARAMS: usize = 16;
/// Longest search query (matches the `web.search` argument schema).
pub const MAX_QUERY_LEN: usize = 256;
/// Most result pages a search form may announce.
pub const MAX_PAGES: u32 = 10;
/// Most web hosts named in a taint panel.
pub const MAX_TAINT_HOSTS: usize = 8;

/// Tier shown on the badge. SAFE never reaches a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DialogTier {
    /// Needs the user's OK.
    Review,
    /// Needs the OK and the typed phrase.
    Dangerous,
}

/// What happens to one listed item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// Moved to the trash (recoverable).
    MoveToTrash,
    /// Deleted for good (emptying the trash).
    DeleteForever,
}

/// One row of the exact list (design.md §6.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    /// Display path, `~`-relative where possible.
    pub path: String,
    /// Size on disk.
    pub size_bytes: u64,
    /// What will happen to it.
    pub kind: ItemKind,
}

/// A validated argument shown verbatim in a marked, read-only box.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Param {
    /// Argument name from the skill's schema.
    pub name: String,
    /// Canonical value.
    pub value: String,
}

/// Why the context is tainted (design.md §6.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaintNote {
    /// Hosts whose content influenced the request; may be empty.
    pub hosts: Vec<String>,
}

/// Something will leave the device (design.md §6.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Egress {
    /// The fixed provider or target host.
    pub provider: String,
    /// The exact text that will be sent.
    pub query: String,
    /// Result pages that will be read afterwards (0 for a single fetch).
    pub pages: u32,
    /// Whether the user may edit `query`.
    pub editable: bool,
}

/// What C6 asks the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialogRequest {
    /// Must equal [`PROTOCOL_VERSION`].
    pub version: u32,
    /// UUIDv7 text of the action, echoed back in the decision.
    pub action_id: String,
    /// Registered capability id.
    pub capability: String,
    /// Badge tier.
    pub tier: DialogTier,
    /// Time left before the request expires.
    pub expires_in_ms: u64,
    /// The approve button stays inactive this long after the window is shown or refocused.
    pub approve_delay_ms: u64,
    /// The exact list of items, when the action has one.
    #[serde(default)]
    pub items: Vec<Item>,
    /// Validated arguments to show.
    #[serde(default)]
    pub params: Vec<Param>,
    /// Present whenever the context is tainted.
    #[serde(default)]
    pub taint: Option<TaintNote>,
    /// Present whenever something leaves the device.
    #[serde(default)]
    pub egress: Option<Egress>,
    /// Phrase the user must type; required for DANGEROUS, forbidden otherwise.
    #[serde(default)]
    pub danger_phrase: Option<String>,
}

/// The user's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// Approve once.
    Allow,
    /// Refuse (button, Esc or closing the window).
    Deny,
    /// The countdown reached zero.
    Expired,
}

/// The dialog's single output line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialogDecision {
    /// Must equal [`PROTOCOL_VERSION`].
    pub version: u32,
    /// Echo of the request's action id.
    pub action_id: String,
    /// The answer.
    pub decision: Decision,
    /// Final query text when the form was editable and the user allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

/// Why a request or decision was refused. Messages never contain the content.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    /// Input larger than the contract allows.
    #[error("message too large")]
    TooLarge,
    /// Not valid JSON for the contract (includes unknown fields).
    #[error("malformed message")]
    Malformed,
    /// Version mismatch.
    #[error("unsupported protocol version")]
    Version,
    /// A field is out of range or has forbidden characters.
    #[error("invalid field `{0}`")]
    Field(&'static str),
    /// The decision answers a different action.
    #[error("decision does not match the action")]
    WrongAction,
}

/// True for text that is safe to render as plain data: no control characters
/// (C0, DEL, C1) and no bidirectional overrides that could disguise a path.
fn plain_text(s: &str, max: usize) -> bool {
    s.len() <= max
        && !s.chars().any(|c| {
            c.is_control()
                || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}')
        })
}

fn host_like(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

fn ident_like(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_')
}

fn action_id_ok(s: &str) -> bool {
    s.len() == 36 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

impl DialogRequest {
    /// Parses and validates a request read from stdin.
    ///
    /// # Errors
    /// Any [`ProtocolError`]; the dialog then denies without showing anything.
    pub fn parse(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(ProtocolError::TooLarge);
        }
        let req: Self = serde_json::from_slice(bytes).map_err(|_| ProtocolError::Malformed)?;
        req.validate()?;
        Ok(req)
    }

    /// Checks every field against the contract.
    ///
    /// # Errors
    /// The first field that is out of range.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        use ProtocolError::Field;
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::Version);
        }
        if !action_id_ok(&self.action_id) {
            return Err(Field("action_id"));
        }
        if !ident_like(&self.capability, 64) {
            return Err(Field("capability"));
        }
        if self.expires_in_ms == 0 || self.expires_in_ms > MAX_EXPIRY_MS {
            return Err(Field("expires_in_ms"));
        }
        if self.approve_delay_ms > MAX_APPROVE_DELAY_MS {
            return Err(Field("approve_delay_ms"));
        }
        if self.items.len() > MAX_ITEMS
            || self
                .items
                .iter()
                .any(|i| i.path.is_empty() || !plain_text(&i.path, 4096))
        {
            return Err(Field("items"));
        }
        if self.params.len() > MAX_PARAMS
            || self
                .params
                .iter()
                .any(|p| !ident_like(&p.name, 64) || !plain_text(&p.value, 4096))
        {
            return Err(Field("params"));
        }
        if let Some(t) = &self.taint
            && (t.hosts.len() > MAX_TAINT_HOSTS || !t.hosts.iter().all(|h| host_like(h)))
        {
            return Err(Field("taint"));
        }
        if let Some(e) = &self.egress
            && (!host_like(&e.provider)
                || e.query.trim().is_empty()
                || !plain_text(&e.query, MAX_QUERY_LEN)
                || e.pages > MAX_PAGES)
        {
            return Err(Field("egress"));
        }
        match (self.tier, &self.danger_phrase) {
            (DialogTier::Dangerous, Some(p)) if !p.trim().is_empty() && plain_text(p, 64) => {}
            (DialogTier::Review, None) => {}
            _ => return Err(Field("danger_phrase")),
        }
        Ok(())
    }
}

impl DialogDecision {
    /// Serialises the decision as one line (no trailing newline).
    ///
    /// # Errors
    /// [`ProtocolError::Malformed`] if serialisation fails.
    pub fn to_line(&self) -> Result<String, ProtocolError> {
        serde_json::to_string(self).map_err(|_| ProtocolError::Malformed)
    }

    /// C6 side: parses the dialog's output and checks it answers `request`.
    /// An edited query is accepted only on an editable egress form and only
    /// with `allow`; it is re-validated like the original.
    ///
    /// # Errors
    /// Any [`ProtocolError`]; C6 then records the action as not approved.
    pub fn parse_for(bytes: &[u8], request: &DialogRequest) -> Result<Self, ProtocolError> {
        if bytes.len() > MAX_DECISION_BYTES {
            return Err(ProtocolError::TooLarge);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| ProtocolError::Malformed)?;
        let d: Self = serde_json::from_str(text.trim_end_matches('\n'))
            .map_err(|_| ProtocolError::Malformed)?;
        if d.version != PROTOCOL_VERSION {
            return Err(ProtocolError::Version);
        }
        if d.action_id != request.action_id {
            return Err(ProtocolError::WrongAction);
        }
        if let Some(q) = &d.query {
            let editable = request.egress.as_ref().is_some_and(|e| e.editable);
            if !editable
                || d.decision != Decision::Allow
                || q.trim().is_empty()
                || !plain_text(q, MAX_QUERY_LEN)
            {
                return Err(ProtocolError::Field("query"));
            }
        }
        Ok(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01934f2a-7b3c-7d4e-8f50-123456789abc";

    fn base() -> DialogRequest {
        DialogRequest {
            version: PROTOCOL_VERSION,
            action_id: ID.into(),
            capability: "system.cleanup".into(),
            tier: DialogTier::Review,
            expires_in_ms: 120_000,
            approve_delay_ms: 1_000,
            items: vec![Item {
                path: "~/.cache/thumbnails".into(),
                size_bytes: 610 << 20,
                kind: ItemKind::MoveToTrash,
            }],
            params: vec![],
            taint: None,
            egress: None,
            danger_phrase: None,
        }
    }

    fn search() -> DialogRequest {
        DialogRequest {
            capability: "web.search".into(),
            items: vec![],
            egress: Some(Egress {
                provider: "search.example".into(),
                query: "shampoo for dry scalp".into(),
                pages: 2,
                editable: true,
            }),
            ..base()
        }
    }

    #[test]
    fn valid_request_round_trips() {
        let req = base();
        let bytes = serde_json::to_vec(&req).unwrap();
        assert_eq!(DialogRequest::parse(&bytes).unwrap(), req);
    }

    #[test]
    fn rejects_bad_requests() {
        type Mutation = Box<dyn Fn(&mut DialogRequest)>;
        let cases: Vec<(&str, Mutation)> = vec![
            ("version", Box::new(|r| r.version = 2)),
            ("action_id", Box::new(|r| r.action_id = "x".into())),
            (
                "capability upper",
                Box::new(|r| r.capability = "System.Cleanup".into()),
            ),
            ("capability empty", Box::new(|r| r.capability.clear())),
            ("expiry zero", Box::new(|r| r.expires_in_ms = 0)),
            (
                "expiry long",
                Box::new(|r| r.expires_in_ms = MAX_EXPIRY_MS + 1),
            ),
            (
                "delay",
                Box::new(|r| r.approve_delay_ms = MAX_APPROVE_DELAY_MS + 1),
            ),
            (
                "item control",
                Box::new(|r| r.items[0].path = "~/a\nb".into()),
            ),
            (
                "item bidi",
                Box::new(|r| r.items[0].path = "~/a\u{202E}gpj.exe".into()),
            ),
            ("item empty", Box::new(|r| r.items[0].path.clear())),
            (
                "too many items",
                Box::new(|r| r.items = vec![r.items[0].clone(); MAX_ITEMS + 1]),
            ),
            (
                "param name",
                Box::new(|r| {
                    r.params = vec![Param {
                        name: "Bad Name".into(),
                        value: "1".into(),
                    }];
                }),
            ),
            (
                "taint host",
                Box::new(|r| {
                    r.taint = Some(TaintNote {
                        hosts: vec!["EVIL.com/x".into()],
                    })
                }),
            ),
            (
                "danger without phrase",
                Box::new(|r| r.tier = DialogTier::Dangerous),
            ),
            (
                "phrase on review",
                Box::new(|r| r.danger_phrase = Some("delete".into())),
            ),
        ];
        for (name, mutate) in cases {
            let mut r = base();
            mutate(&mut r);
            assert!(r.validate().is_err(), "{name} should be rejected");
        }
    }

    #[test]
    fn rejects_bad_egress() {
        for mutate in [
            (|e: &mut Egress| e.query = "   ".into()) as fn(&mut Egress),
            |e| e.query = "a".repeat(MAX_QUERY_LEN + 1),
            |e| e.query = "a\u{7}".into(),
            |e| e.provider = "Search.Example".into(),
            |e| e.pages = MAX_PAGES + 1,
        ] {
            let mut r = search();
            if let Some(e) = r.egress.as_mut() {
                mutate(e);
            }
            assert!(r.validate().is_err());
        }
    }

    #[test]
    fn rejects_unknown_fields_and_size() {
        let mut v = serde_json::to_value(base()).unwrap();
        v["extra"] = serde_json::json!(1);
        assert_eq!(
            DialogRequest::parse(&serde_json::to_vec(&v).unwrap()),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            DialogRequest::parse(&vec![b' '; MAX_REQUEST_BYTES + 1]),
            Err(ProtocolError::TooLarge)
        );
        assert_eq!(
            DialogRequest::parse(b"not json"),
            Err(ProtocolError::Malformed)
        );
    }

    #[test]
    fn dangerous_with_phrase_is_valid() {
        let mut r = base();
        r.tier = DialogTier::Dangerous;
        r.danger_phrase = Some("run it".into());
        assert!(r.validate().is_ok());
    }

    fn decision(d: Decision, query: Option<&str>) -> Vec<u8> {
        DialogDecision {
            version: PROTOCOL_VERSION,
            action_id: ID.into(),
            decision: d,
            query: query.map(str::to_owned),
        }
        .to_line()
        .unwrap()
        .into_bytes()
    }

    #[test]
    fn decision_checks() {
        let req = base();
        for d in [Decision::Allow, Decision::Deny, Decision::Expired] {
            assert_eq!(
                DialogDecision::parse_for(&decision(d, None), &req)
                    .unwrap()
                    .decision,
                d
            );
        }
        // A query on a non-egress form is refused.
        assert!(DialogDecision::parse_for(&decision(Decision::Allow, Some("x")), &req).is_err());
        // Wrong action id.
        let mut other = req.clone();
        other.action_id = "01934f2a-7b3c-7d4e-8f50-000000000000".into();
        assert_eq!(
            DialogDecision::parse_for(&decision(Decision::Allow, None), &other),
            Err(ProtocolError::WrongAction)
        );
        assert_eq!(
            DialogDecision::parse_for(b"", &req),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            DialogDecision::parse_for(&vec![b'x'; MAX_DECISION_BYTES + 1], &req),
            Err(ProtocolError::TooLarge)
        );
    }

    #[test]
    fn edited_query_rules() {
        let req = search();
        let ok =
            DialogDecision::parse_for(&decision(Decision::Allow, Some("dry scalp shampoo")), &req)
                .unwrap();
        assert_eq!(ok.query.as_deref(), Some("dry scalp shampoo"));
        // Query with deny, empty query, control chars, or a non-editable form: refused.
        assert!(DialogDecision::parse_for(&decision(Decision::Deny, Some("x")), &req).is_err());
        assert!(DialogDecision::parse_for(&decision(Decision::Allow, Some("  ")), &req).is_err());
        assert!(
            DialogDecision::parse_for(&decision(Decision::Allow, Some("a\u{1b}[2J")), &req)
                .is_err()
        );
        let mut fixed = req.clone();
        if let Some(e) = fixed.egress.as_mut() {
            e.editable = false;
        }
        assert!(DialogDecision::parse_for(&decision(Decision::Allow, Some("x")), &fixed).is_err());
    }
}
