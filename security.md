# security.md — Security and Privacy Plan

**Document version:** 2.0 · **Date:** 2026-11-14 · **Status:** Baseline for the rebuild
**Scope:** what we protect, from whom, with which controls, and how each control is proven.
**Mechanisms** (unit settings, Landlock tables, tier logic, approval protocol, log formats, SSRF lists) are specified in [Architecture.md](Architecture.md) §8. This document explains why they exist and how they are verified.

---

## 1. What We Protect

| # | Asset | Why it matters |
| :--- | :--- | :--- |
| A-1 | Activity memory (window titles, files, builds, media, voice questions) | A detailed record of the user's life at the keyboard. |
| A-2 | Personal facts | Things the user chose to tell the buddy about themselves. |
| A-3 | Live audio | The microphone is open while the service runs. |
| A-4 | The power to act | Skills can delete files, write notes and reach the network. |
| A-5 | The approval keys | Whoever holds them can forge "the user said yes". |
| A-6 | The audit log and egress ledger | The user's proof of what happened. |
| A-7 | The user's attention and trust | A buddy that speaks first can be turned into a channel for manipulation. |

## 2. Who We Defend Against

| # | Adversary | Capability | In scope |
| :--- | :--- | :--- | :--- |
| T-1 | Malicious web content | Controls the text of any page or search result the fetcher brings in. | **Yes — primary** |
| T-2 | Malicious local content | Controls window titles, file names, media metadata, note text, calendar entries (for example a web page sets a tab title). | **Yes** |
| T-3 | Hostile network | Controls DNS answers and servers the fetcher talks to. | **Yes** |
| T-4 | Hostile audio | Speech from speakers, videos or people nearby. | **Yes** |
| T-5 | A compromised OctoPlus part | Code execution inside one sandboxed service, for example through a parser bug. | **Yes — contain it** |
| T-6 | A bad dependency or model file | Supply-chain tampering. | **Yes — detect and limit** |
| T-7 | Another local user | A different account on the machine. | **Yes** |
| T-8 | Malware running as the same user | Can read the user's files directly. | **No** — see §9 |
| T-9 | Physical access, root, kernel compromise | — | **No** — see §9 |

## 3. Security Objectives

| # | Objective |
| :--- | :--- |
| SO-1 | Nothing reaches the network except one approved request at a time, and every byte out is recorded. |
| SO-2 | Outside content can never cause an action, a stored fact, or a spoken nudge without the user's confirmation. |
| SO-3 | Approval cannot be forged, replayed, or given by anything but the user's hand on the dialog. |
| SO-4 | A break-in to one part does not give access to the others' data, keys or sockets beyond that part's declared needs. |
| SO-5 | Every action and every outbound request leaves a tamper-evident record. |
| SO-6 | The user can pause, inspect, forget and purge, and those commands do what they say. |
| SO-7 | The buddy never speaks or shows private details to bystanders by default. |

---

## 4. Threats and Controls by Boundary

STRIDE letters: **S**poofing, **T**ampering, **R**epudiation, **I**nformation disclosure, **D**enial of service, **E**levation of privilege.

### 4.1 Internet ↔ fetcher (Zone 0 ↔ Zone 1)

| Threat | Type | Control |
| :--- | :--- | :--- |
| Fetcher used to reach local or private services (SSRF, DNS rebinding, redirects) | E, I | Resolve once, block all non-public ranges, pin the connection, re-validate each redirect; kernel network rule allows TCP 443 only. |
| Server sends huge, slow or odd content | D | Size, time and content-type limits; streaming parse with caps. |
| Request reveals who the user is | I | No cookies, no stored state, no referrer, fixed generic user agent; only the approved text is sent. |
| Parser bug in HTML or TLS handling gives code execution | E | Fetcher is a separate user with its own sandbox, no access to the user's home, runtime folder or keys; memory-safe language; fuzzing. |
| A compromised fetcher fetches on its own | E, I | It has nothing private to send: it cannot read user data. Residual: it could contact the network without approval — detected by comparing packet capture with the ledger (KPI-11), and bounded by its sandbox. |

### 4.2 Fetcher → storage (Zone 1 → Zone 2, the spool)

| Threat | Type | Control |
| :--- | :--- | :--- |
| Malformed spool file attacks the storage parser | T, E | Strict schema, size limits, UTF-8 validation; reject and delete on any failure. |
| Web content stored as if it were trusted | T, E | Every spool ingest is forced to `EXTERNAL_UNTRUSTED` by C3 regardless of what the file claims. |
| Taint lost later (merge, de-duplication, summarising) | E | Taint union on every merge; missing taint is an error; property tests. |
| Web content lives forever in memory | I, E | Web documents expire (PRD §6.5). |

### 4.3 Content → prompt → model (inside Zone 2)

| Threat | Type | Control |
| :--- | :--- | :--- |
| Injected instructions in web pages **or in local text** such as window titles | E | The model's output cannot act: skills run only via a grammar-limited call that C6 evaluates; tainted context raises tiers. |
| Injected model control tokens break out of the prompt structure | T | Control-token sequences are neutralised in every piece of text not written by us. |
| Injected text makes the buddy say something harmful or false | S, T | Tainted text is wrapped and labelled; answers using web content say so; the dialog never shows model text. Residual: the spoken answer can still be wrong — see §9. |
| Injected text plants a "personal fact" | T, E | Facts are saved only on the user's word; under taint, saving needs the dialog. |
| Injected text triggers a nudge | S | Nudges are templates and numbers from first-party health samples only; the engine never reads evidence text. |
| Injected text asks the user to approve something | S | The dialog states plainly what will happen and why confirmation is needed, including "this request was influenced by web content". |

### 4.4 Knowledge → kernel (Zone 2 → Zone 3)

| Threat | Type | Control |
| :--- | :--- | :--- |
| A compromised C5 asks for dangerous actions | E | C6 decides tiers from its own registry; anything beyond SAFE needs the user. |
| A compromised C5 lies about taint to stay SAFE | E | Residual, bounded: SAFE skills are read-only or user-requested by design, and every dispatch is audited. Hardening: C6 asks C3 directly for the taint of referenced chunks when the request names them. |
| Skill arguments carry a path or command trick | T, E | Per-skill argument schema; canonical paths; per-skill sandbox that cannot reach outside its allowlist even if the arguments lie. |
| Flood of requests to wear the user down ("dialog fatigue") | D, S | One dialog at a time; rate limit on REVIEW requests; repeated denials suppress identical requests for a cooling period. |

### 4.5 Kernel ↔ user (the approval boundary)

| Threat | Type | Control |
| :--- | :--- | :--- |
| Forged or replayed approval | S | HMAC-bound token over action, arguments, decision and expiry; single-use nonce; constant-time check; executor re-verifies. |
| Another program fakes the dialog or reads the decision | S | The dialog is C6's own child; the decision returns over a private pipe. Residual: a look-alike window from same-user malware is out of scope (§9). |
| The user approves by accident | S | Deny has default focus; Enter never approves; the Allow button activates only after a short delay; DANGEROUS needs a typed phrase. |
| Approval by voice from speakers or a bystander | S | There is no voice path to approval. |
| The dialog shows something different from what runs | T | The token binds the hash of the final arguments; an edit in the dialog re-binds before anything runs. |

### 4.6 Microphone and speaker

| Threat | Type | Control |
| :--- | :--- | :--- |
| Always-listening microphone becomes a recorder | I | Audio lives only in a bounded memory buffer; transcription only after the wake word or inside a follow-up window; nothing written to disk; verified by file-watch test. |
| Capture attaches to the wrong device | I | Connect only to the exact configured node; if it cannot be resolved, capture nothing. |
| Audio from speakers issues commands | S, E | Own playback is ignored; hands-free commands can run SAFE skills only; anything else needs the dialog. |
| The buddy speaks private details aloud | I | Generic nudge wording by default; hold when the user is away, locked or fullscreen; notify-only mode; answers never read raw paths, hashes or URLs. |

### 4.7 Same-user parts against each other

| Threat | Type | Control |
| :--- | :--- | :--- |
| A compromised part opens another part's socket | E | Each unit's mount namespace contains only the socket folders it may use; servers also check peer UID and peer unit. |
| A compromised part reads the database, the keys or the logs | I | Per-unit home masking plus Landlock: only C3 sees `storage/`, only C6 sees `hmac.key` and the logs. |
| A compromised part debugs or signals another | E | `NoNewPrivileges`, restricted ptrace, no capabilities, restricted syscalls. |
| Crash dumps or swap leak content | I | Core dumps disabled; keys held in locked, zeroised memory. |

### 4.8 Supply chain

| Threat | Type | Control |
| :--- | :--- | :--- |
| Tampered model file | T | SHA-256 in the manifest, checked at fetch and at every start. |
| Model file format bug exploited at load | E | Models come only from the manifest; the inference unit has no network, no home access and a hard memory limit. |
| Bad or vulnerable dependency | T, E | Everything pinned; `cargo-deny` for advisories, licences, sources and the network-crate ban; submodules pinned by commit; ONNX Runtime pinned by hash. |
| Network code slips into an isolated part | I | Build fails if any crate except the fetcher links an HTTP or DNS client; the kernel blocks it regardless. |

---

## 5. Isolation Design in Plain Terms

Three walls stand between the internet and the user's data:

1. **A different user.** The fetcher is the only process with a network, and it runs as its own system user that cannot read the user's home or runtime folder.
2. **A one-way door.** The fetcher can only drop files in a spool. Storage reads them as untrusted. The only path the other way is a signed request from the kernel.
3. **A human gate.** That signed request exists only after the user clicked Allow on a dialog showing the exact text to be sent.

Inside the user's own services, isolation is layered: each unit has no network; each unit sees only its own files and its allowed peers' sockets; each process then locks itself down further before reading any input.

**Honest limit:** the user-side services share one UID. Our controls stop a *sandboxed* part from reaching the others. They do not stop an unrelated program the user runs from reading `~/.local/share/octoplus/` directly. That threat is handled by the operating system, not by us (§9).

---

## 6. Key Management

| Key | Purpose | Location and access | Rotation |
| :--- | :--- | :--- | :--- |
| `hmac.key` (32 random bytes) | Binds approvals for skills run by the kernel | `~/.config/octoplus/hmac.key`, 0600, visible only in C6's unit | Regenerated on install and by `octoplusctl rotate-keys` |
| `fetch.key` (32 random bytes) | Binds approvals for fetch and search requests; shared between C6 and C7 | C6 copy beside `hmac.key`; C7 copy readable only by `octoplus-fetcher` | Same command, both copies together |

- The two keys are different, so a break-in to the fetcher never reveals the key that approves local skills.
- Keys are created from the OS random source, loaded into zeroised memory, never logged, never sent over IPC.
- Rotation invalidates all outstanding tokens; tokens live 120 s at most.

---

## 7. Privacy Plan

| Principle | How it is met |
| :--- | :--- |
| **Collect little** | No screenshots, keystrokes or stored audio. Excluded apps and paused periods never leave the monitor. The noise gate drops most events before storage. |
| **Keep it local** | One database under the user's home. No network for any part that can read it. |
| **Keep it briefly** | Retention per PRD §6.5; a hard cap on stored chunks. |
| **Show what left** | The egress ledger lists every outbound request with its exact text. |
| **Give control** | Pause, exclusion list, forget by time or app, profile list and remove, purge everything. |
| **Protect bystander privacy** | Generic nudges, hold rules, notify-only mode. |
| **No surprises in logs** | No user content at `info` or above; arguments hashed in the audit log. |
| **Safe test data** | Recorded fixtures are anonymised with a keyed HMAC and a discarded per-run key before they are ever committed. Raw recordings never enter the repository. |

**Data at rest** is not encrypted by OctoPlus. The install guide tells the user to enable full-disk encryption, and `octoplusctl status` warns once if the home filesystem is not on an encrypted volume.

---

## 8. Verification Plan

A control without a test is a hope. Each row is an exit criterion of the phase named.

| Control | How it is proven | Phase |
| :--- | :--- | :--- |
| No network in isolated units | Inside each real unit: TCP connect, DNS lookup and raw socket all fail; local sockets still work (`local_sc_proof_1_net`). Every running part has its own network namespace with only `lo` (`scripts/check-egress.sh`; a part in the host namespace is a failure). Every user unit has exactly one exec (generator test), since a second exec drops `PrivateNetwork=` in user scope (ADR-0002 addendum) | 1, re-run in 9 |
| Socket map enforced | In each part's exact unit, every socket folder outside its row of the map is invisible (ENOENT), every one in it visible; expectations come from `socketmap` (`local_sc_proof_2_socket_map`, negative control: an extra bind is detected). Second layer (server peer-unit check): `octoplus-ipc` peer tests | 1, 9 |
| Landlock applied before input | Each real part (Rust and C++) reads a canary visible in its unit but outside its policy right after `restrict`; healthd must report `landlock_canary_denied` for all of them, and `cat` of the same file in each unit's exact namespace without Landlock must succeed (`local_sc_proof_3_landlock`). No shipped binary can skip `restrict` (test-only feature, BUG-025) | 1 onward |
| Network-crate ban | A throwaway probe crate triggers the ban | 0 |
| Tier logic | Every tier × taint × disabled combination; property test that effective tier ≥ base tier | 2 |
| Approval protocol | Valid, expired, replayed, wrong arguments, wrong capability, bit-flipped token; forged approval from another process | 2 |
| Dialog safety | Enter does not approve; Allow delay; edit re-binds the hash | 2 |
| Skill sandbox | Each skill tries to read `hmac.key`, write outside its allowlist, open a socket; all fail | 2 |
| Audit and ledger integrity | Tampered line detected; a failed `ATTEMPT` write denies the action | 2, 8 |
| Anonymiser | A placeholder cannot be reproduced without the key; two runs never match. Proven by `octoplus-monitor` `tests/anonymise_test.rs`: keyed placeholder is not a plain hash, sentinel text in every text field never survives (property test, with a negative control that shows the check can fail), the key is not in the dump bytes, the dump is mode 0600 and never overwrites a file | 3 |
| Exclusion and pause | Property test over random window, process-tree, sample, pause, resume and clock sequences against an independent model of the pause timer: excluded apps, excluded title markers and everything during a pause never leave (`tests/privacy_test.rs`, with a negative control that shows a leaky gate is caught); a socket-level test shows the same through a real subscriber. The property test found and fixed a leak where a window focused during a pause could be missed after resume | 3 |
| Monitor surface | C1's unit sees the compositor socket and the session bus read-only, never the system bus or the accessibility bus; no other unit sees either (`octoplus-deploy` unit tests). Sensor text from other programs (media metadata) is stripped of control characters and capped; file watching never follows symlinks and never reads file contents; a stalled subscriber is cut loose without slowing any sensor (`tests/foundation_test.rs`) | 3 |
| Taint preservation | Property tests through ingest, merge, de-duplication, query and prompt assembly. Phase 5: the prompt's taint is the union of every input even when a block is dropped or cut (`prompt_test`: dropped hostile block still taints); C3 evidence or a fact with a missing or unknown taint is dropped and counted, never treated as clean (`evidence_test`, `turn_test`); the session's whole taint rides into a later dispatch (`turn_test::external_taint_in_the_session_carries_into_a_later_action`) | 4, 5 |
| Forget | After forget: zero rows, zero text-index hits, zero vectors; gone from the next backup | 4 |
| Control-token neutralisation | Corpus of control sequences in titles, notes, questions, evidence. Phase 5: an 85-line corpus (`tests/fixtures/control_tokens.txt`: ChatML, Qwen, Llama, Gemma markers, zero-width and fullwidth forms, fake closing tags, 10,000 repeated `<`) is inert after `neutralise`, idempotent, and every corpus line placed in every prompt slot leaves exactly three ChatML markers and at most one wrapper pair (`sanitize_test`, `prompt_test`); a negative control shows a naive neutraliser fails it. Our own persona block mentions the wrapper tag, so tests count markers after it | 5 |
| Injection resistance | Corpus of injected pages and local strings through the real path: zero unauthorised executions, zero unconfirmed saved facts. Phase 5: 100 hostile strings (25 each in evidence, question, saved fact and window title; 22 attack classes) through the whole turn engine against fake C3, C4 and C6 where the model answers every action prompt with `shell.exec rm -rf ~`: nothing outside the closed skill table is ever dispatched, no hostile data slot triggers any dispatch, no `profile.remember` happens, the prompt scaffolding holds (`turn_test::injection_corpus_...`, with a negative control). Fake peers only: the real model's behaviour on these strings is measured in the integration phase | 5, re-run in 8 and 9 |
| Nudge isolation | Static check and test: the engine has no code path to evidence text or to `Dispatch`. Phase 6: `nudge_isolation_test` scans every source file under `src/nudge/` and fails on any mention of the dispatch, generate, evidence, fact or forget messages or any `crate::` path except `router` (negative control included); the engine's only outgoing text-bearing message is `NotifyRequest { template_id, args: (name, f64), silent }`, and the test also fails if the engine file contains `format!(`; the offered skill is a name that only the turn engine can dispatch, from the user's reply, after checking it against the rule file's fixed list; `engine` tests assert zero dispatches from any number of ticks | 6 |
| Nudge hold rules | Replay fixtures: zero nudges spoken during call, media, fullscreen, pause, quiet hours. Phase 6: one scenario per bad moment (paused, quiet hours, turn in progress, microphone busy, media, fullscreen, away) with voice available and a rule firing the whole time: 0 spoken, each nudge held or sent as a notification (`tests/fixtures/nudge-scenarios/kpi10_*.json`); and the three recorded dumps replayed through the real signal builder and core, 178 ticks each, 55 to 81 bad moments, 3 spoken at good ones and none at a bad one (`nudge_dump_replay_test`). Mic-busy and call detection arrive with voice (Phase 7), so the trial log re-checks them | 6, 7 |
| No audio on disk | File-watch over home and temp during a full voice session. Phase 7: `scripts/voice-sc-checks.sh` on the installed stack: during a spoken exchange no audio file (by extension) appears under home or the temp folders and the live process holds no file descriptor open for writing; the Landlock policy grants write access only to the socket folders and the PipeWire socket (`liboctoplus` `ASingleFileCanBeGrantedAndItsNeighbourStaysClosed`); `octoplus-voice` logs counts only, never text (the optional `OCTOPLUS_VOICE_TRACE` prints event names) | 7 |
| Capture device strictness | Configured node missing → no capture stream exists. Phase 7: `LocalPipeWire.AMissingMicrophoneMeansNoCaptureAtAll` (adapter) and `scripts/voice-sc-checks.sh` (installed unit with a nonexistent `capture_node`: the service reports it is not listening and `pw-cli` shows no `octoplus-voice-capture` node); an unplugged device is reported (`AnUnpluggedMicrophoneIsReported`) | 7 |
| No nudge spoken while another app holds the microphone | Phase 7: the microphone probe sees another application's running capture stream (`TheMicrophoneProbeSeesAnotherAppCapturing`); C5 holds or notifies instead of speaking (`another_app_on_the_microphone_or_a_turn_in_progress_means_no_speech`); C2 itself refuses a nudge during a turn (`ANudgeIsRefusedWhileATurnIsInProgress`) | 7 |
| Voice has no approval path | A transcript is only text to C5; the turn engine treats it like a typed question (same taint and kernel check); dialogs are answered on screen by C6 only (R0-8). `VoiceCommand` carries text, times and a flag, nothing that selects a tier | 7 |
| SSRF policy | Corpus: every blocked range, encoded addresses, rebinding resolver, redirects to private targets, odd schemes; URL-parser fuzzing ≥ 1 h | 8 |
| Fetcher identity | A non-kernel peer and a request without a valid token are rejected | 8 |
| Request hygiene | Captured request has no cookies, referrer or identifying headers; text equals the approved text | 8 |
| Egress accounting | Packet capture during the whole suite matches the ledger exactly; isolated units send 0 bytes | 9 |
| Unit hardening score | `systemd-analyze security` for every unit; documented exceptions only | 9 |
| Fault behaviour | Disk full, killed dialog, killed kernel mid-approval → deny, never allow | 2, 9 |

---

## 9. Out of Scope and Residual Risk

Stated plainly so nobody relies on a protection that is not there.

| # | Item | Position |
| :--- | :--- | :--- |
| RR-1 | Malware running as the user can read the database and imitate windows | Not defended at the application level. Mitigation is OS-level: full-disk encryption, a clean system. |
| RR-2 | Root, kernel or physical compromise | Not defended. |
| RR-3 | The model can say wrong things | Reduced by honesty rules and the lookup offer, never eliminated. No professional advice. |
| RR-4 | A spoken answer built on web content may repeat false claims from that content | The answer says it came from the web; actions remain gated. |
| RR-5 | The search provider learns the approved query and the machine's IP address | Inherent to searching. The user sees and can edit the query first. |
| RR-6 | A compromised C5 could under-report taint for SAFE skills | SAFE skills are read-only or user-requested; all dispatches audited; cross-check with C3 where the request names chunks. |
| RR-7 | A compromised fetcher can make requests nobody approved | It holds no private data; detected by capture-versus-ledger comparison; bounded by its sandbox. |
| RR-8 | The wake word can be triggered by other audio | Only SAFE skills run hands-free; nothing is approved by voice. |
| RR-9 | Third-party model and audio runtimes are large C++ code bases | Contained by sandboxing, no network, memory limits; pinned and hash-checked. |
| RR-11 | Window titles, app ids and media tags are personal and are sent raw on C1's stream to C3 (C1 never filters, AB-5) | Only the storage unit may subscribe (peer-unit check). Excluded apps and paused periods never leave C1. Recordings are anonymised before they touch disk. Lock state is not observable on COSMIC inside C1's allowed surface (ADR-0011), so it is reported as unknown and the nudge policy holds |
| RR-12 | OctoPit is a window like any other; same-user malware can imitate it | OctoPit has no approval control and says so on screen; approvals happen only in C6's own dialog, so a look-alike OctoPit gains nothing. It holds no keys, opens no network, records no audio. |
| RR-10 | With optional GPU offload on, C4 talks to a large proprietary GPU kernel driver | Only C4, only when enabled (off by default); C4 has no network, no home access and no user data on disk; device access limited to an allowlist (ADR-0008). Not present on the CPU path. |

---

## 10. Secure Development Practice

1. **Security is built first.** Sandboxes and the socket map exist in Phase 1, the safety kernel in Phase 2. Features are added behind them.
2. **Every story touching a boundary** updates this document in the same change and adds or extends a row in §8.
3. **Builder and checker are different.** Whoever writes a change does not sign it off ([rules.md](rules.md) §2).
4. **Fail closed** in all security code; fail soft only in user-experience code.
5. **Corpora grow with every finding.** A security bug is fixed by first adding the input that triggers it to the injection, SSRF or control-token corpus.
6. **Dependencies:** pinned, reviewed on change, checked on every CI run. An advisory exception needs a written reason, a revisit condition and the owner's approval.
7. **Secrets and private data never enter the repository:** keys, models, raw recordings, real transcripts.
8. **When something goes wrong:** record it in `BUGS.md` with severity; for anything touching SO-1 to SO-3, stop feature work, fix, add the regression test, and note it in the phase report.
