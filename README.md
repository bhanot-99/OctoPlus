# Semantic-NeuroOS

**A Sovereign, Ambient, Zero-Egress Cognitive Operating System Companion for Linux.**

> *"A buddy that lives in your laptop: it notices things, remembers things, talks like a person, and nothing leaves your machine unless you say yes."*

---

## Table of Contents

- [1. Executive Overview](#1-executive-overview)
- [2. Core Architectural Philosophy & Privacy Invariants](#2-core-architectural-philosophy--privacy-invariants)
- [3. System Architecture & Trust Zones](#3-system-architecture--trust-zones)
- [4. Component Catalogue](#4-component-catalogue)
- [5. Inter-Process Communication (IPC) Fabric](#5-inter-process-communication-ipc-fabric)
- [6. Deep Dive: Subsystems & Daemons](#6-deep-dive-subsystems--daemons)
  - [C1: Context & Health Monitor (`neuroos-monitor`)](#c1-context--health-monitor-neuroos-monitor)
  - [C2: Voice & Audio Pipeline (`neuroos-voice`)](#c2-voice--audio-pipeline-neuroos-voice)
  - [C3: Semantic Storage & In-Memory Vector Search (`neuroos-storage`)](#c3-semantic-storage--in-memory-vector-search-neuroos-storage)
  - [C4: Local Model Inference (`neuroos-inference`)](#c4-local-model-inference-neuroos-inference)
  - [C5: Knowledge Orchestration & Nudge Engine (`neuroos-knowledge`)](#c5-knowledge-orchestration--nudge-engine-neuroos-knowledge)
  - [C6: Safety Kernel & Human Confirmation (`neuroos-kernel` & `neuroos-confirm`)](#c6-safety-kernel--human-confirmation-neuroos-kernel--neuroos-confirm)
  - [C7: Controlled Egress DMZ (`neuroos-fetcher`)](#c7-controlled-egress-dmz-neuroos-fetcher)
  - [D: System Health Watchdog (`neuroos-healthd`)](#d-system-health-watchdog-neuroos-healthd)
  - [Operator CLI (`neuroosctl`)](#operator-cli-neuroosctl)
- [7. Data Model & Storage Schema](#7-data-model--storage-schema)
- [8. Security Model, Taint Propagation & Sandboxing](#8-security-model-taint-propagation--sandboxing)
- [9. Models & Hardware Accelerators](#9-models--hardware-accelerators)
- [10. Platform Requirements](#10-platform-requirements)
- [11. Build, Installation & Quickstart](#11-build-installation--quickstart)
- [12. CLI Operations Guide (`neuroosctl`)](#12-cli-operations-guide-neuroosctl)
- [13. Development, Verification & Testing Gates](#13-development-verification--testing-gates)
- [14. Project Roadmap & Delivery Status](#14-project-roadmap--delivery-status)
- [15. Companion Specifications](#15-companion-specifications)

---

## 1. Executive Overview

**Semantic-NeuroOS** is a fully local, privacy-first ambient assistant designed for modern Linux workstations and laptops running Wayland (COSMIC / wlroots), PipeWire, and systemd.

Unlike cloud-tethered assistants that stream personal data, keystrokes, and audio to remote endpoints, Semantic-NeuroOS operates under a **zero-egress-by-construction** guarantee:

1. **Senses Continuously**: Non-intrusively samples desktop window focus, user idle state, media playback, git activity, and physical hardware telemetry (NVMe/SATA disk usage, CPU/memory consumption, thermal sensors, battery drain).
2. **Remembers Locally**: Distills transient telemetry into long-term semantic memory stored in a single embedded SQLite database with exact in-memory vector embeddings and FTS5 full-text search.
3. **Speaks Proactively (Nudges)**: Evaluates background telemetry using deterministic, hysteresis-governed rules to alert you *before* problems occur (e.g., thermal throttling, battery depletion, disk saturation, break reminders).
4. **Understands Natural Language**: Resolves deictic references (e.g., *"What error was on my screen just now?"*) by joining spoken timestamps against structured focus history.
5. **Answers with One Consistent Voice**: Streams speech responses sentence-by-sentence with sub-100ms hardware barge-in cancellation and conversational follow-up windows.
6. **Guarantees Privacy & Safety**: Every internal service is denied network access by the Linux kernel. A single sandboxed egress DMZ runs under a separate system user and can execute only one approved web search at a time, strictly verified by cryptographic HMAC tokens and out-of-process GTK4 human confirmations.

---

## 2. Core Architectural Philosophy & Privacy Invariants

The design of Semantic-NeuroOS is governed by ten inviolable architectural principles:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         CORE ARCHITECTURAL PRINCIPLES                       │
├──────┬───────────────────────────────┬──────────────────────────────────────┤
│ AP-1 │ No Egress by Construction     │ Kernel-enforced isolation. Only C7   │
│      │                               │ has network namespace access.        │
├──────┼───────────────────────────────┼──────────────────────────────────────┤
│ AP-2 │ Isolated Unix Processes       │ Seven services + healthd communicate │
│      │                               │ solely via versioned protobuf IPC.   │
├──────┼───────────────────────────────┼──────────────────────────────────────┤
│ AP-3 │ Single Resource Ownership     │ C3 owns storage; C4 owns LLM; C6 owns│
│      │                               │ HMAC key, audit logs & UI dialogs.   │
├──────┼───────────────────────────────┼──────────────────────────────────────┤
│ AP-4 │ Least Privilege (Enforced 2x) │ systemd unit restrictions + Landlock │
│      │                               │ ABI v4 applied at startup.           │
├──────┼───────────────────────────────┼──────────────────────────────────────┤
│ AP-5 │ Monotonic Taint Growth        │ External data stays marked untrusted │
│      │                               │ across merges, summaries, and tools. │
├──────┼───────────────────────────────┼──────────────────────────────────────┤
│ AP-6 │ Propose, Never Decide         │ LLM outputs text/grammar only. Skill │
│      │                               │ actions require C6 safety gating.    │
├──────┼───────────────────────────────┼──────────────────────────────────────┤
│ AP-7 │ Simplicity Before Cleverness  │ Single SQLite WAL database, exact    │
│      │                               │ in-memory scan, no extra engines.    │
├──────┼───────────────────────────────┼──────────────────────────────────────┤
│ AP-8 │ Strict Latency Budgets        │ Every hop has budgeted deadlines; slow│
│      │                               │ work never sits on the audio path.   │
├──────┼───────────────────────────────┼──────────────────────────────────────┤
│ AP-9 │ Graceful Degradation          │ Losing a service drops a feature, not│
│      │                               │ the whole operating system session.  │
├──────┼───────────────────────────────┼──────────────────────────────────────┤
│AP-10 │ External Observability        │ Scraped by an out-of-band daemon;    │
│      │                               │ verified by tamper-evident logs.     │
└──────┴───────────────────────────────┴──────────────────────────────────────┘
```

---

## 3. System Architecture & Trust Zones

Isolation in Semantic-NeuroOS is organized into four concentric **Trust Zones**:

```mermaid
flowchart TB
    subgraph Z0["Zone 0 — Untrusted Outside World"]
        WEB[(The Internet)]
    end

    subgraph Z1["Zone 1 — Egress DMZ (Dedicated System User)"]
        C7["C7: neuroos-fetcher<br/>(User: neuroos-fetcher)"]
        SPOOL[("/var/spool/neuroos-fetcher/<br/>(Spool Directory)")]
    end

    subgraph Z2["Zone 2 — Isolated User Services (PrivateNetwork=true)"]
        C1["C1: neuroos-monitor<br/>(Sensors & Telemetry)"]
        C2["C2: neuroos-voice<br/>(PipeWire, VAD, ASR, TTS)"]
        C3["C3: neuroos-storage<br/>(SQLite, In-Memory Vectors)"]
        C4["C4: neuroos-inference<br/>(llama.cpp Runtime)"]
        C5["C5: neuroos-knowledge<br/>(Turn Router & Nudge Engine)"]
        D["D: neuroos-healthd<br/>(Watchdog & Soak Monitor)"]
    end

    subgraph Z3["Zone 3 — Decision Core (Strictest Sandboxing)"]
        C6["C6: neuroos-kernel<br/>(HMAC Key, Audit Ledger)"]
        UI["neuroos-confirm<br/>(Out-of-Process GTK4 Dialog)"]
        SR["neuroos-sandbox<br/>(Child Skill Runner)"]
    end

    WEB <-->|HTTPS GET / TCP 443| C7
    C7 -->|Drop Untrusted JSON| SPOOL
    SPOOL -->|Read & Ingest with Taint| C3
    C6 -->|HMAC-Approved Request| C7
    C5 -->|Dispatch Intent| C6
    C6 -->|Spawn Isolated Skill| SR
    C6 -->|Invoke Confirmation| UI
    C2 <-->|VoiceCommand / Spoken Output| C5
    C5 <-->|Generate Text Stream| C4
    C5 <-->|Hybrid Search / Telemetry| C3
    C1 -->|Raw Event Stream| C3
    D -.->|Scrape /health.sock| C1
    D -.->|Scrape /health.sock| C2
    D -.->|Scrape /health.sock| C3
    D -.->|Scrape /health.sock| C4
    D -.->|Scrape /health.sock| C5
    D -.->|Scrape /health.sock| C6
    D -.->|Scrape /health.sock| C7
```

### Boundary Security Rules
1. **Zone 1 → Zone 2**: Data crosses exclusively via the file spool (`/var/spool/neuroos-fetcher/`). Incoming documents are forcibly assigned the `EXTERNAL_UNTRUSTED` taint flag by C3.
2. **Zone 2 → Zone 1**: Outbound requests are initiated exclusively by C6 upon human approval, cryptographically sealed with a single-use HMAC token (`fetch.key`).
3. **Zone 3 Execution**: Skills run exclusively under `neuroos-sandbox` in dedicated Landlock/seccomp child processes. Visual confirmation dialogs run out-of-process via GTK4.
4. **UID Separation**: Zone 1 runs under `neuroos-fetcher`. Zones 2 and 3 run under the desktop user's UID, isolated from each other via systemd mount namespaces (`TemporaryFileSystem=%t/neuroos` + explicit `BindPaths=`) and Landlock ABI v4 rulesets.

---

## 4. Component Catalogue

| Component | Service Binary | Language | Runtime Scope | Network Isolation | Primary Responsibilities & Ownership |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **C1** | `neuroos-monitor` | Rust | User unit | `PrivateNetwork=true` | Monitors Wayland focus, idle timeouts, MPRIS media, process hierarchy, system resources, and thermals. |
| **C2** | `neuroos-voice` | C++20 | User unit | `PrivateNetwork=true` | Captures audio via PipeWire, Silero VAD, openWakeWord ("hey jarvis"), whisper.cpp ASR, and Kokoro fp32 TTS. Hardware barge-in support. |
| **C3** | `neuroos-storage` | Rust | User unit | `PrivateNetwork=true` | Owns all persistent data in one SQLite database (`neuroos.sqlite3`), in-memory dot-product vector matrix, FTS5 lexical index, and ONNX Runtime embedding. |
| **C4** | `neuroos-inference` | C++20 | User unit | `PrivateNetwork=true` | Language model inference using upstream llama.cpp (`Qwen3-4B-Instruct-2507 Q4_K_M`), KV-cache prompt reuse, and BNF grammar enforcement. |
| **C5** | `neuroos-knowledge` | Rust | User unit | `PrivateNetwork=true` | Turn routing, prompt assembly, session context, deictic resolution, token neutralizer, and the proactive **Nudge Engine**. |
| **C6** | `neuroos-kernel` | Rust / GTK4 | User unit | `PrivateNetwork=true` | Authority boundary: holds `hmac.key`, evaluates capability tiers, spawns `neuroos-confirm` GTK4 dialogs, runs sandboxed skills, and maintains hash-chained audit ledgers. |
| **C7** | `neuroos-fetcher` | Rust | System unit | **TCP 443 Only** (Zone 1) | Dedicated user `neuroos-fetcher`. Token-verified web search & fetch, anti-SSRF IP filtering, DNS pinning, and spool delivery. |
| **D** | `neuroos-healthd` | Rust | User unit | `PrivateNetwork=true` | Out-of-band watchdog; scrapes metrics across all component `/health.sock` endpoints, monitors cgroups, and logs soak stability. |
| **CLI**| `neuroosctl` | Rust | User CLI | Inherits shell | Interactive operator tool for status inspection, chat, skill execution, storage lifecycle jobs, nudge control, and audit verification. |

---

## 5. Inter-Process Communication (IPC) Fabric

All inter-process communication in Semantic-NeuroOS takes place over **Unix Domain Sockets** using filesystem paths. Abstract sockets and loopback TCP/UDP interfaces are strictly banned.

### 5.1 Enforced Socket Map

Runtime root: `$XDG_RUNTIME_DIR/neuroos/` (permissions `0700`). Every socket occupies its own isolated subdirectory (e.g. `<service>/<socket>/<socket>.sock`) (ADR-0009). Systemd unit files generate restricted mount namespaces where each service binds **only** the subdirectories of the peers it is authorized to call:

```
$XDG_RUNTIME_DIR/neuroos/
├── monitor/
│   ├── stream/stream.sock      # C1 -> C3 (raw event stream)
│   └── control/control.sock    # C1 -> C6, neuroosctl
├── voice/
│   ├── voice/voice.sock        # C2 -> C5 (audio commands & speech chunks)
│   └── control/control.sock    # C2 -> C6, neuroosctl
├── storage/
│   └── storage/storage.sock    # C3 -> C5, C6, neuroosctl
├── inference/
│   └── inference/inference.sock# C4 -> C5 (prompt generation stream)
├── knowledge/
│   └── knowledge/knowledge.sock# C5 -> C2, neuroosctl
├── kernel/
│   └── kernel/kernel.sock      # C6 -> C5, neuroosctl
├── healthd/
│   └── healthd/healthd.sock    # healthd -> neuroosctl
└── <component>/health/health.sock # Scraped exclusively by healthd
```

*Note: C7 fetcher sockets reside in `/run/neuroos-fetcher/` owned by group `neuroos` (permissions `0660`).*

### 5.2 Framing & Wire Envelope

Messages are length-delimited with a **4-byte little-endian unsigned integer** specifying frame payload size, followed by a serialized Protocol Buffers `neuroos.v1.Envelope`:

```protobuf
syntax = "proto3";
package neuroos.v1;

message Envelope {
  uint32 schema_version = 1;  // Contract version
  string trace_id       = 2;  // Trace ID propagated end-to-end
  uint64 request_id     = 3;  // Request ID (0 for push events)
  uint64 sent_at_ns     = 4;  // UTC epoch nanoseconds
  bool   end_of_stream  = 5;  // Terminal frame indicator for streams
  oneof body {
    // 10-19: Common / Ping / Pong / Error
    // 20-39: C1 Monitor events
    // 40-59: C2 Voice commands & audio status
    // 60-99: C3 Storage queries & lifecycle jobs
    // 100-119: C4 Inference prompt & text chunks
    // 120-159: C5 Knowledge turns & nudge triggers
    // 160-199: C6 Kernel evaluate, dispatch & confirmations
    // 200-219: C7 Fetcher search & fetch requests
    // 220-239: D Health status & telemetry reports
  }
}
```

### 5.3 Streaming & Peer Authentication
- **Token Streaming**: Text generated by C4 streams to C5 and C2 as sequential sentence frames with an explicit `end_of_stream = true` marker.
- **Deadlines**: Requests carry strict latency deadlines (e.g. 150 ms for vector queries, 800 ms for wake response).
- **Kernel Peer Checks**: Servers verify peer identity using `SO_PEERCRED` (verifying UID) and validate the caller's systemd unit by reading `/proc/<pid>/cgroup`.

---

## 6. Deep Dive: Subsystems & Daemons

### C1: Context & Health Monitor (`neuroos-monitor`)
Written in Rust. C1 gathers continuous context without screen recording, keylogging, or OCR.
- **Sensor Workers**:
  - **Window Focus**: Tracks active Wayland window `app_id`, title, PID, and focus spans via native COSMIC / wlroots protocols. Excluded windows emit redacted events (empty title/app) to break context cleanly.
  - **User Idle**: Samples Wayland idle status (60 s default timeout) and screen lock state.
  - **Media State**: Monitors MPRIS D-Bus interfaces for media playback, title, artist, and track changes.
  - **Hardware Telemetry**: Probes `/proc` and `/sys` to sample CPU load, memory consumption, highest-memory process, NVMe/SATA filesystem capacities, battery percentage/charge state, and CPU package thermal sensors.
- **Exclusion & Privacy Pause**: Emits nothing during pause mode, followed by a state snapshot upon resumption.
- **Ring Buffers & Replay**: Maintains bounded drop-oldest queues. Supports `--record <file>` to generate HMAC-anonymized dumps for offline regression replay via `neuroosctl replay`.

### C2: Voice & Audio Pipeline (`neuroos-voice`)
Written in C++20 for optimal audio timing and zero-copy signal processing.
- **Capture**: Connects to PipeWire for 16 kHz mono capture. Binds strictly to a designated audio node id.
- **VAD & Wake**: Silero VAD runs continuously (< 3% CPU). openWakeWord triggers on the wake phrase (`"hey jarvis"`).
- **Speech-to-Text**: whisper.cpp (`tiny.en`) transcribes spoken queries immediately following end-of-speech silence (≥ 600 ms).
- **Text-to-Speech**: Kokoro fp32 ONNX synthesizes natural speech sentence-by-sentence in the consistent voice `af_heart`. Preambles (e.g., *"Looking that up..."*) and nudge openers are pre-rendered at startup.
- **Hardware Barge-In**: Immediate audio playback flush within 100 ms when the user speaks over the assistant, instantly notifying C5 to cancel C4 inference.
- **Follow-up Window**: Keeps the microphone open for 8 seconds after speaking to allow fluid conversation without repeating the wake phrase.

### C3: Semantic Storage & In-Memory Vector Search (`neuroos-storage`)
Written in Rust. C3 is the sole custodian of persistent user data.
- **Single SQLite Database**: Operates in WAL mode with `synchronous=NORMAL` and `auto_vacuum=INCREMENTAL`.
- **4-Stage Ingest Noise Filter**:
  1. *Subprocess Collapse*: Groups child build jobs or sub-shells under their root window process.
  2. *Self Filtering*: Drops NeuroOS dialogs and notifications.
  3. *Promotion Gate*: Requires ≥ 3 occurrences or > 5 s of active dwell time to prevent transient window flicker from polluting memory.
  4. *Media Demotion*: Down-ranks background audio/video playback unless explicitly interacted with.
- **Exact Vector Matrix**: Embeds promoted text chunks into 384-dimensional vectors using local `bge-small-en-v1.5` via ONNX Runtime 1.31.1. Vectors are held in a contiguous in-memory float array for sub-5ms exact dot-product scanning (no approximate ANN graph drift).
- **Hybrid Retrieval**: Combines SQLite FTS5 BM25 text rank and dot-product vector scores using **Reciprocal Rank Fusion (RRF)**.
- **Data Lifecycle & Cryptographic Forget**: Daily vacuum GC, hourly telemetry rollups, and atomic snapshot backups (keeps 8 newest). `forget` commands execute in a single ACID transaction, overwriting chunks via `secure_delete` and scrubbing existing backups immediately.

### C4: Local Model Inference (`neuroos-inference`)
Written in C++20, wrapping upstream `llama.cpp`.
- **Model**: Executes `Qwen3-4B-Instruct-2507` (Q4_K_M quantized GGUF).
- **Prompt Cache**: Persists KV-cache across conversational turns for stable prefix blocks (persona, safety constraints, skill definitions).
- **Grammar Constrained Output**: Enforces strict BNF grammars during skill extraction turns to guarantee syntactically valid JSON arguments.
- **Compute Accelerators**: Optimized for AVX2 CPU execution by default (4.2 s p50 response). Optional Vulkan GPU offload on supported hardware (RTX 3050 Mobile VRAM budget: 3,100 MiB).

### C5: Knowledge Orchestration & Nudge Engine (`neuroos-knowledge`)
Written in Rust. C5 manages conversational state and contextual awareness.
- **Deictic Resolution**: Resolves references like *"this"*, *"that"*, or *"the build failure"* by retrieving the window focus containing the timestamp `t_speech_start`.
- **Prompt Assembly**: Fits turns into a strict 384-token budget:
  - 128 tokens: Persona, rules, and skill definitions (cached).
  - 48 tokens: Long-term personal user facts (`profile.remember`).
  - 32 tokens: Resolved desktop focus context.
  - 48 tokens: Multi-turn conversational session history.
  - 128 tokens: Retrieved evidence chunks (wrapped in boundary markers).
- **Deterministic Turn Router**: Evaluates input sequentially:
  1. *Nudge Reply*: Handled with active nudge context if within the follow-up window.
  2. *Action Dispatch*: Grammar-constrained skill formulation sent to C6.
  3. *Desktop/Memory Query*: Grounded answer synthesis using C3 evidence.
  4. *General Conversation*: Direct conversational response.
- **Proactive Nudge Engine**: Evaluates background telemetry every 30 s against TOML rules (disk, battery, thermal, memory, quiet hours). Incorporates hysteresis, debounce windows, and a context policy gate (suppressing speech during calls, fullscreen gaming, or media playback).

### C6: Safety Kernel & Human Confirmation (`neuroos-kernel` & `neuroos-confirm`)
Written in Rust and GTK4. C6 enforces the capability authorization boundary.
- **Capability Tiers**:
  - `SAFE`: Read-only actions (e.g. `system.disk_report`, `memory.search`). Run without confirmation when untainted.
  - `REVIEW`: Sensitive operations (e.g. `system.cleanup`, `web.search`, `notes.append`). Always require explicit human confirmation.
  - `DANGEROUS`: Destructive actions. Require confirmation and a typed verification phrase.
- **Dynamic Taint Escalation**: If context includes `EXTERNAL_UNTRUSTED` data (from web lookups or untrusted window titles), normally `SAFE` operations automatically escalate to `REVIEW`.
- **Out-of-Process GTK4 Confirmation Dialog (`neuroos-confirm`)**:
  - Renders completely outside the requesting service.
  - Deny button has default focus; pressing Enter cannot approve.
  - Allow button activates only after a deliberate human delay.
  - Destructive operations require typing a specific confirmation phrase.
  - Voice approval is architecturally impossible.
- **Cryptographic Nonce Token**: Approvals generate a single-use HMAC-SHA256 token binding `action_id`, `capability`, `args_hash`, and a 120-second expiration.
- **Isolated Skill Runner (`neuroos-sandbox`)**: Executes child skills inside dedicated Landlock sandboxes and seccomp-bpf filter jails.
- **Hash-Chained Audit Ledgers**: Every action generates tamper-evident JSON lines in `actions.jsonl` (and `egress.jsonl` for network access) with SHA-256 rolling block chains.

### C7: Controlled Egress DMZ (`neuroos-fetcher`)
Written in Rust. C7 is the only process on the machine with network namespace access.
- **Dedicated System User**: Runs under the isolated system account `neuroos-fetcher` (group `neuroos`).
- **Authorization Enforcement**: Accepts requests exclusively from C6 containing valid HMAC signatures (`fetch.key`).
- **Anti-SSRF Protection**: Resolves addresses once; aggressively blocks RFC 1918, loopback (`127.0.0.0/8`), carrier-grade NAT (`100.64.0.0/10`), link-local (`169.254.0.0/16`), multicast, and IPv6 unmapped ranges. Pins validated IPs for TLS connections.
- **Single-Turn Execution**: Fetches exactly one approved search or approved URL at a time (up to 3 redirects, re-verified).
- **Spool Output**: Writes clean, size-capped JSON to `/var/spool/neuroos-fetcher/` and immediately exits. It has no read access to user data.

### D: System Health Watchdog (`neuroos-healthd`)
Written in Rust. Runs out-of-band to monitor system integrity.
- Scrapes `/health.sock` on all services every 30 seconds.
- Gathers RSS memory usage, latency distributions, and error counters from Linux cgroups.
- Maintains soak stability history and generates telemetry reports for `neuroosctl status`.

---

## 7. Data Model & Storage Schema

All persistent user data is maintained in a single SQLite database (`~/.local/share/neuroos/storage/neuroos.sqlite3`):

```sql
-- Tracked window focus segments
CREATE TABLE focus_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    app_id TEXT NOT NULL,
    window_title TEXT NOT NULL,
    pid INTEGER NOT NULL,
    started_at_ns INTEGER NOT NULL,
    ended_at_ns INTEGER,
    dwell_ms INTEGER,
    taint_flags INTEGER NOT NULL DEFAULT 0
);

-- Promoted semantic entities
CREATE TABLE entities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    domain INTEGER NOT NULL,
    kind TEXT NOT NULL,
    label TEXT NOT NULL,
    taint_flags INTEGER NOT NULL,
    first_seen_ns INTEGER NOT NULL,
    last_seen_ns INTEGER NOT NULL
);

-- Searchable text chunks & embedding vectors
CREATE TABLE chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_id INTEGER REFERENCES entities(id) ON DELETE CASCADE,
    domain INTEGER NOT NULL,
    chunk_text TEXT NOT NULL,
    taint_flags INTEGER NOT NULL,
    created_at_ns INTEGER NOT NULL,
    embedding BLOB -- 384x float32 binary blob
);

-- Full-text lexical search
CREATE VIRTUAL TABLE chunks_fts USING fts5(
    chunk_text,
    content='chunks',
    content_rowid='id'
);

-- Numeric machine health time series
CREATE TABLE health_samples (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    metric_id INTEGER NOT NULL,
    value REAL NOT NULL,
    sampled_at_ns INTEGER NOT NULL
);

-- User-declared personal facts
CREATE TABLE profile_facts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    fact_text TEXT NOT NULL,
    category TEXT NOT NULL,
    source TEXT NOT NULL,
    created_at_ns INTEGER NOT NULL,
    last_used_at_ns INTEGER
);

-- History of proactive nudges
CREATE TABLE nudge_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_id TEXT NOT NULL,
    delivery_mode INTEGER NOT NULL,
    triggered_at_ns INTEGER NOT NULL,
    outcome INTEGER NOT NULL
);
```

---

## 8. Security Model, Taint Propagation & Sandboxing

### 8.1 Monotonic Taint Propagation

Taint flags track the provenance of all data moving through the system:

```rust
bitflags! {
    pub struct TaintFlags: u32 {
        const EXTERNAL_UNTRUSTED = 1 << 0; // Ingested from C7 fetcher
        const USER_PROVIDED      = 1 << 1; // Directly entered by user
        const MODEL_GENERATED    = 1 << 2; // Produced by local LLM
        const STALE_INDEX        = 1 << 3; // Embedding model changed
    }
}
```

- Invariant: `taint(output) = union(taint(inputs))`
- Taint can never be cleared or reduced by summarization, deduplication, or multi-hop retrieval.
- Any evidence containing `EXTERNAL_UNTRUSTED` forces downstream skill evaluations into `REVIEW` tier.

### 8.2 Defense-in-Depth Sandbox Matrix

```
┌───────────────────┬──────────────┬───────────────┬────────────────────────────────────┐
│ Component         │ Network      │ Landlock (RO) │ Landlock (RW) / Bindings           │
├───────────────────┼──────────────┼───────────────┼────────────────────────────────────┤
│ C1 Monitor        │ None         │ /proc, /sys,  │ Own runtime socket folder          │
│                   │              │ Wayland, D-Bus│                                    │
├───────────────────┼──────────────┼───────────────┼────────────────────────────────────┤
│ C2 Voice          │ None         │ Voice models, │ Own runtime socket folder          │
│                   │              │ PipeWire conf │                                    │
├───────────────────┼──────────────┼───────────────┼────────────────────────────────────┤
│ C3 Storage        │ None         │ ONNX model,   │ SQLite db, backups, own runtime    │
│                   │              │ Spool (RO+del)│ socket folder                      │
├───────────────────┼──────────────┼───────────────┼────────────────────────────────────┤
│ C4 Inference      │ None         │ LLM GGUF,     │ Own runtime socket folder          │
│                   │              │ /dev/nvidia*  │                                    │
├───────────────────┼──────────────┼───────────────┼────────────────────────────────────┤
│ C5 Knowledge      │ None         │ Prompts,      │ Own runtime socket folder          │
│                   │              │ nudges.toml   │                                    │
├───────────────────┼──────────────┼───────────────┼────────────────────────────────────┤
│ C6 Kernel         │ None         │ hmac.key,     │ Audit ledgers, own runtime folder  │
│                   │              │ Wayland/GTK   │                                    │
├───────────────────┼──────────────┼───────────────┼────────────────────────────────────┤
│ C7 Fetcher        │ TCP 443 only │ TLS CA certs  │ Spool dir, /run/neuroos-fetcher    │
├───────────────────┼──────────────┼───────────────┼────────────────────────────────────┤
│ Skill Sandbox     │ None         │ System libs   │ Per-skill allowlisted targets only │
└───────────────────┴──────────────┴───────────────┴────────────────────────────────────┘
```

---

## 9. Models & Hardware Accelerators

All machine learning models are verified by SHA-256 checksums in [`models/manifest.toml`](file:///home/bhanot/Semantic%20NeuroOS/models/manifest.toml) before loading. Models are never fetched dynamically at runtime.

| Role | Model ID | Artifact / Format | Size | License | Purpose |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **LLM** | `qwen3-4b-instruct-2507` | `Qwen3-4B-Instruct-2507-Q4_K_M.gguf` | 2.50 GB | Apache-2.0 | Conversational answering, context synthesis, skill argument extraction. |
| **Embedding** | `bge-small-en-v1.5` | `bge-small-en-v1.5.onnx` (384-dim) | 133 MB | MIT | Vector representations for C3 semantic memory. |
| **ASR (STT)**| `whisper-tiny-en` | `ggml-tiny.en.bin` | 77.7 MB | MIT | Local speech-to-text transcription. |
| **VAD** | `silero-vad-v6.2.3` | `silero_vad.onnx` | 2.3 MB | MIT | Voice activity detection (< 3% CPU). |
| **Wake Word**| `openwakeword-jarvis` | `hey_jarvis_v0.1.onnx` + mel models | ~5 MB | Apache / CC | On-device wake-word detection. |
| **TTS** | `kokoro-v1.0-fp32` | `kokoro-v1.0-fp32.onnx` (`af_heart`) | 326 MB | Apache-2.0 | High-fidelity local speech synthesis. |
| **Runtime** | `onnxruntime-1.31.1`| `libonnxruntime.so.1.31.1` | 11.4 MB | MIT | Pinned dynamic runtime for ONNX inference. |

### Optional GPU Acceleration (C4)
Semantic-NeuroOS runs fully on modern AVX2 CPUs. On workstations with dedicated NVIDIA GPUs (e.g. RTX 3050 Mobile), inference offload can be enabled via the Vulkan backend (`BindPaths=/dev/nvidia0 /dev/nvidiactl`, VRAM budget capped at 3,100 MiB) to achieve sub-3s answer generation.

---

## 10. Platform Requirements

### Target Hardware
- **Processor**: x86_64 CPU with AVX2 support (4 cores minimum; 8 cores / 16 threads recommended).
- **Memory**: 16 GB RAM recommended (8 GB supported if model RSS ≤ 1,600 MiB).
- **Disk**: 8 GB free storage space on an SSD/NVMe volume.
- **Microphone & Speakers**: Configured under PipeWire.

### Operating System & Dependencies
- **OS**: Linux kernel ≥ 6.7 (mandatory for Landlock ABI v4 network restrictions).
- **Display Server**: Wayland compositor (COSMIC Desktop, Sway, or wlroots).
- **Audio Server**: PipeWire ≥ 1.0 with PipeWire Pulse compatibility layer.
- **Init System**: systemd ≥ 255 (user service isolation with `TemporaryFileSystem=`).
- **Toolchains**: Rust 1.84+ (edition 2024), GCC 13+ / Clang 17+ (C++20), CMake 3.25+, Ninja, `just`, `protoc`.

---

## 11. Build, Installation & Quickstart

Semantic-NeuroOS uses `just` as its single entry point.

### Step 1: Install Development Dependencies

On Ubuntu 24.04 / Pop!_OS 24.04:
```bash
sudo apt-get update && sudo apt-get install -y \
    build-essential cmake ninja-build protobuf-compiler \
    libpipewire-0.3-dev libgtk-4-dev libsqlite3-dev \
    libespeak-ng-dev libasound2-dev libseccomp-dev
```

### Step 2: Fetch and Verify Verified Models

Download and verify all SHA-256 pinned models:
```bash
just fetch-models
```

### Step 3: Build the Complete Stack

Build all Rust workspace crates, C++ libraries, and voice pipelines:
```bash
just build
```

### Step 4: Install System Units & Configuration

Install user-level units into `~/.local/` and configure the fetcher user:
```bash
# 1. Generate systemd unit files from the enforced socket map
just units

# 2. Install developer binaries and user services
just install-dev

# 3. Provision the isolated fetcher system user and spool permissions
sudo scripts/install-fetcher.sh
```

### Step 5: Start Semantic-NeuroOS

Enable and start the user target:
```bash
systemctl --user daemon-reload
systemctl --user enable --now neuroos.target
```

Inspect operational status:
```bash
neuroosctl status
```

---

## 12. CLI Operations Guide (`neuroosctl`)

`neuroosctl` is the operator CLI for managing, inspecting, and querying Semantic-NeuroOS.

### Health & Telemetry
```bash
# Inspect health, RSS, and socket connectivity across all services
neuroosctl status

# Emit structured machine-readable JSON status
neuroosctl status --json
```

### Ask Questions (CLI Mode)
```bash
# Ask a question via the knowledge and inference pipeline
neuroosctl ask "What did I work on this morning?"

# Display turn routing, evidence retrieval, and taint flags
neuroosctl ask "Why is my laptop fan spinning?" --explain
```

### Nudge Engine Management
```bash
# List all proactive nudge rules and their cooldown states
neuroosctl nudges list

# Temporarily disable or enable a nudge rule
neuroosctl nudges disable disk_space_warning
neuroosctl nudges enable disk_space_warning

# Test-fire a nudge notification or speech sample
neuroosctl nudges test disk_space_warning --voice
```

### Memory & Storage Operations
```bash
# Search persistent memory using hybrid semantic retrieval
neuroosctl storage search "compile error in rustix" --top-k 3

# Trigger immediate background storage lifecycle jobs
neuroosctl storage job gc
neuroosctl storage job backup
neuroosctl storage job rollup
```

### Personal Facts & Memory
```bash
# View remembered facts about user preferences
neuroosctl profile list

# Explicitly store or remove a personal preference
neuroosctl profile add "I prefer dark mode in all applications"
neuroosctl profile remove 1
```

### Security & Audit Verification
```bash
# Inspect recent actions recorded by the safety kernel
neuroosctl audit list --limit 10

# Cryptographically verify the hash-chained audit ledger
neuroosctl audit verify

# Inspect outbound network requests in the egress ledger
neuroosctl egress list
```

### Telemetry Replay & Harness
```bash
# Replay an anonymized monitor dump at 2x real-time speed
neuroosctl replay dump-20261116.dump --speed 2.0
```

---

## 13. Development, Verification & Testing Gates

Every story and pull request must satisfy the automated gate pipeline:

```bash
# Run the complete test suite (Rust unit tests + C++ ctest)
just test

# Check code formatting (Rustfmt + clang-format)
just fmt-check

# Run linters (clippy, clang-tidy, shellcheck, cargo-machete)
just lint

# Validate dependency licenses and ban policies
just deny

# Measure code coverage (Rust >= 80%, C++ >= 70%)
just cov

# Execute C++ tests under AddressSanitizer and ThreadSanitizer
just sanitize

# Run the three security proofs and verify egress isolation
just sc
```

---

## 14. Project Roadmap & Delivery Status

Development is delivered across 11 structured phases:

| Phase | Description | Status | Deliverables & Milestones |
| :---: | :--- | :---: | :--- |
| **0** | **Spikes & Foundations** | ✅ Done | Architecture baseline, SQLite FTS5/vector scan spike, model bake-off (`Qwen3-4B`), systemd Landlock hardening. |
| **1** | **Walking Skeleton** | ✅ Done | All 7 services running under `neuroos.target`, IPC framing, `healthd` watchdog, initial security proofs. |
| **2** | **Safety Kernel (C6)** | ✅ Done | `hmac.key` binding, GTK4 dialog (`neuroos-confirm`), capability registry, hash-chained audit ledgers. |
| **3** | **Context Monitor (C1)** | ✅ Done | Wayland focus tracking, idle/lock sensors, hardware sampling, privacy pause, anonymized dump recorder. |
| **4** | **Semantic Storage (C3)** | ✅ Done | SQLite WAL database, 4-stage noise filter, `bge-small-en-v1.5` exact vector scan, cryptographic forget/purge. |
| **5** | **Knowledge & Chat (C5)** | ✅ Done | Turn routing, deictic resolution, 384-token prompt assembler, control-token neutralizer, session history. |
| **6** | **Nudge Engine** | ✅ Done | Background rule evaluator (disk, battery, thermal, memory), cooldowns, quiet hours, context policy gate. |
| **7** | **Voice Pipeline (C2)** | ✅ Done | PipeWire 16 kHz capture, Silero VAD, openWakeWord, whisper.cpp ASR, Kokoro fp32 TTS (`af_heart`), barge-in. |
| **8** | **Controlled Fetcher (C7)** | ✅ Done | Zone 1 egress daemon, SSRF protection, token verification, web search provider integration. |
| **9** | **Integration & Soak** | ✅ Done | Full 24-hour multi-modal soak, fault injection & chaos resilience, live Wayland compositor recovery tests. |
| **10** | **Packaging & Release** | ✅ Done | Signed `.deb` distribution, production installer/uninstaller, `neuroosctl init`, operator runbooks, v1.0.0. |

---

## 15. Companion Specifications

For detailed implementation and design documents, refer to the project specifications:

- [`PRD.md`](PRD.md): Product requirements, numeric latency targets, and resource budgets.
- [`Architecture.md`](Architecture.md): Complete system architecture, IPC contracts, and component designs.
- [`security.md`](security.md): Threat model (STRIDE), cryptographic protocols, and sandboxing proofs.
- [`design.md`](design.md): Persona voice guidelines, confirmation dialog UX, and nudge rule definitions.
- [`rules.md`](rules.md): Binding engineering rules, quality standards, and Definition of Done.
- [`memory.md`](memory.md): Current active sprint state, recent decisions, and open architectural queries.
- [`docs/adr/`](docs/adr/): Architectural Decision Records (ADR-0001 through ADR-0015).
- [`BUGS.md`](BUGS.md): Known issues, root cause analyses, and tracked resolutions.

---

## License

Semantic-NeuroOS is licensed under dual [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE) terms, with specific exceptions for model weights and third-party submodules documented in [`models/manifest.toml`](models/manifest.toml) and [`deny.toml`](deny.toml).
