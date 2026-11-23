# ADR-0017 — OctoPit, the desktop dashboard

**Status:** Accepted 2026-11-23 (owner, D-57)
**Date:** 2026-11-23
**Changes:** `PRD.md` §3.2 (graphical app), `rules.md` AB-4 and the free-text rule, `security.md` RR-12, `Architecture.md` §5.2 (socket map: client class `OctoPit`), §13 (packaging), `docs/octo-plus-vision.md` §5.
**Evidence:** owner brief (a proper GPU dashboard as the face of Semantic NeuroOS); `docs/plans/phase-11-octopit-plan.md`; `docs/design/octopit-ux.md`.

---

## Context

v1.0 is voice, notifications, one dialog and a CLI. The owner wants a professional desktop face for the system: health, chat, a mic button, backends and GPU, activity and privacy. GTK4 is already in the workspace for the C6 dialog.

## Decision

1. A new program `neuroos-octopit` (Rust, GTK4), run by the user. It is a client like `neuroosctl`, not a sandboxed part. It talks only to the sockets listed for it in the socket map (`Client::OctoPit`): healthd, knowledge, voice/control, kernel.
2. It has no Approve control. A pending approval is shown as "waiting for your approval in the dialog".
3. Model text is shown as plain text only: no markup, links, images or remote content; control characters removed; length capped.
4. No network crate, no web view, no microphone access. The mic button sends start/stop to C2's control socket; C2 owns all audio.
5. C2 gains control messages (start listening, stop listening, cancel) and a level value in its status. They only open or close the same capture path the wake word opens.
6. C1 gains an optional GPU health row (additive).
7. OctoPit's logic lives in a GTK-free library crate so most tests need no display.

## Consequences

Good: one place to see the whole system; the privacy page proves the zero-egress claim. Bad: a new client of the control sockets, so the socket-map tests and the security probe grow; the "only C6 shows anything" rule is relaxed for status and chat. Cost that cannot be made zero: same-user malware can imitate the window (RR-12); it gains no approval path.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| Slint / Metal / SwiftUI (vision doc) | New dependencies and licences for no gain on the Linux target |
| Web view or Electron | Large attack surface, remote content risk |
| Reuse `Client::Ctl` | Hides OctoPit's reach; a separate class lists exactly what each page may call |
