# OctoPlus: The Sovereign Cognitive Operating System

**Document Type:** Vision, Architecture & Master Roadmap RFC  
**System Designation:** `OctoPlus` (Next-Generation AI Operating System)  
**File Location:** `docs/octo-plus-vision.md`  
**Status:** Vision, not approved scope (2026-11-23). Several items conflict with the PRD scope and with rules R0-1, R0-7 and AB-4 and each needs its own decision record (ADR) before any work starts.  

---

## 1. Executive Summary

**OctoPlus** is a sovereign, spatially-aware cognitive operating system that integrates local systems architecture, novel neural mechanics, hybrid frontier intelligence, and an encrypted mobile remote link into a single, unified computing organism.

Inspired by the cephalopod nervous system—where a central cognitive core works in harmony with semi-autonomous, highly adaptive limbs—**OctoPlus** blends:
1. **The Physical Anatomy:** The deterministic, kernel-isolated daemons of **OctoPlus**.
2. **The Biological Brain:** The ultra-compact, compute-reusing recurrent core of **PhaseLoopNN**.
3. **Spatial Awareness:** Ambient physical dwell tracking and location-based proactive suggestions via GPS.
4. **Orbital Cloud Uplink:** A cryptographically audited, sanitized bridge to frontier cloud models (**Claude**).
5. **Open-Source Agent Mesh:** Sandboxed autonomous agents executing specialized system and research tasks.
6. **Unified Hermetic Packaging:** Single-bundle distribution (`.app` / `.deb`) with a native GUI Cockpit.
7. **Mobile Neural Link:** End-to-end encrypted peer-to-peer control of your laptop from anywhere on Earth.

---

## 2. High-Level System Architecture

```mermaid
flowchart TD
    subgraph PhysicalWorld ["Physical & Spatial Senses"]
        GPS["GPS / Physical Dwell"]
        Mic["Microphone / Audio (PipeWire / CoreAudio)"]
        Display["Desktop Focus (Wayland / NSWorkspace)"]
    end

    subgraph OctoCore ["OctoPlus Core Substrate (Local Machine)"]
        direction TB
        OctoPlus["OctoPlus Substrate<br/><i>(Sensing · Vector Storage · Safety Kernel)</i>"]
        PhaseLoop["PhaseLoopNN Cognitive Core<br/><i>(Always-on System 1 Instinct · < 50MB RAM)</i>"]
        AgentMesh["Open-Source Agent Mesh<br/><i>(Code Janitor · Research Scout · System Physician)</i>"]
        
        OctoPlus <--> PhaseLoop
        OctoPlus <--> AgentMesh
    end

    subgraph SecurityGate ["Safety & Audit Boundary"]
        Gate["C6 Safety Kernel & Cryptographic Egress Ledger"]
    end

    subgraph Uplink ["Frontier Cloud Uplink (Optional)"]
        Claude["Claude (Anthropic API)<br/><i>Orbital Reasoning on Sanitized Requests</i>"]
    end

    subgraph UserSurfaces ["User Interfaces"]
        Cockpit["OctoPlus Cockpit (Native GUI Dashboard)"]
        Mobile["Mobile Neural Link (E2E Encrypted Phone App)"]
    end

    PhysicalWorld --> OctoPlus
    OctoCore --> Gate
    Gate -- "Approved Outbound Tokens" --> Claude
    OctoCore <--> Cockpit
    OctoCore <== "Noise Protocol / WireGuard P2P" ==> Mobile
```

---

## 3. The Core Pillars of OctoPlus

### Pillar 1: The OS Substrate (OctoPlus)
* **Zero-Network Isolation by Default:** Every core daemon touches user data under strict kernel isolation (`Landlock ABI v4` on Linux, `Seatbelt / App Sandbox` on macOS).
* **Episodic Memory Matrix:** SQLite WAL database storing full-text search (FTS5) alongside dense 384-dimensional vector embeddings (`bge-small-en-v1.5`), strictly capped at 30,000 chunks (≤ 2 GB disk).
* **Deterministic Safety Kernel (C6):** Actions are categorized into `SAFE`, `REVIEW`, and `DANGEROUS` tiers. Destructive actions require explicit visual confirmation dialogs.

### Pillar 2: The Biological Neural Core (PhaseLoopNN)
* **Decoupled Thinking and Knowing:** Separates compute-heavy logic from encyclopedic memory.
* **Recurrent Think Block:** Loops 2 to 6 times at each word, achieving the reasoning depth of a 5× to 10× larger model with a memory footprint under **50 MB of RAM**.
* **State & Order Tracking:** Uses clock-hand turning and mirror-flip reading lines to track chronological user workflows without token-scrambling.
* **Continuous Ambient Cognition:** Runs inside the CPU L3 hardware cache 24/7 without fan noise, thermal throttling, or battery drain.

### Pillar 3: Real-Time Spatial & Physical Awareness
* **Physical Dwell Tracking:** Combines screen focus tracking with GPS coordinates (via CoreLocation / GeoClue) to understand real-world context (e.g., home, office, cafe, transit).
* **Proactive Geospatial Suggestions:** Suggests nearby spots (e.g., quiet workspaces, coffee shops) based on physical location and personal habits.
* **Geotagged Episodic Memory:** Recalls past work by physical location (*"What was the git branch I worked on at the library on Friday?"*).

### Pillar 4: Sovereign Orbital Uplink (Secure Claude Bridge)
* **Isolated Egress DMZ:** All external network access is restricted to the dedicated Fetcher daemon (`C7`).
* **Sanitized Token Payloads:** When a task exceeds local parameter capacity, OctoPlus automatically strips private credentials and personal identifiers.
* **Explicit User Approval:** A visual confirmation prompt shows the exact prompt tokens and endpoint before any data leaves the device.
* **Cryptographic Egress Ledger:** Every outbound byte is logged to an immutable, inspectable audit trail.

### Pillar 5: Open-Source Agent Mesh
* **Specialized Autonomous Limbs:** Micro-agents running in restricted child sandboxes:
  * **Code Janitor:** Analyzes git diffs, catches build bugs, and pre-runs test suites.
  * **Intelligence Scout:** Performs verified web lookups and document extraction without ad trackers.
  * **System Physician:** Monitors thermals, prunes build caches, and optimizes power states.

---

## 4. Hermetic Native Packaging

Rather than an open folder of loosely managed scripts, **OctoPlus is packaged as a monolithic, self-contained distribution**:

* **macOS Distribution (`OctoPlus.app` / `.dmg`):**
  * Single drag-and-drop installation into `/Applications`.
  * Automatically provisions background `launchd` LaunchAgents.
  * Self-verifying SHA-256 integrity checks for all pinned model files on first launch.
  * Sandboxed user storage under `~/.octo/`.
* **Linux Distribution (`.deb` / Flatpak):**
  * Single-command setup configuring systemd user units, permission profiles, and socket directories.
* **Zero External Dependencies:** Ships with bundled ONNX Runtime, embedded SQLite, and compiled C++/Rust binaries.

---

## 5. The OctoPlus Cockpit (Native GUI Dashboard)

> **Status 2026-11-23:** built as **OctoPit** with GTK4 (not Slint), as a client that cannot approve anything: ADR-0017, `docs/design/octopit-ux.md`. The 3D Memory Galaxy and the cognitive dials below are not built.

A sleek, hardware-accelerated dashboard (built with Slint / Metal / SwiftUI) that provides full transparency into the operating system:

1. **System Vitals & Neural Load:**
   * Live gauges for CPU, Metal GPU, Neural Engine (NPU) usage, thermal headrooms, and battery draw.
2. **The 3D "Memory Galaxy":**
   * An interactive visualization of episodic memories, active window nodes, and personal knowledge clusters stored in SQLite. Users can click any node to inspect, search, or purge it.
3. **Cryptographic Firewall & Egress Radar:**
   * Real-time visual ledger showing network traffic (which remains zero until an approved fetch occurs).
4. **Cognitive Performance Dials:**
   * Real-time metrics tracking speech-to-text latency, PhaseLoopNN loop settling counts, and token generation speeds.

---

## 6. Interaction Modalities: Hands-Free Voice & Stealth Silent Chat

OctoPlus adapts dynamically to the user's physical environment, offering two interchangeable interaction paradigms that share the exact same cognitive memory and desktop context:

### 6.1 Hands-Free Voice Mode (Private & Open Environments)
* **Trigger:** Wake word (*"Hey Jarvis"* / *"Hey Octo"*).
* **Delivery:** Full-duplex voice streaming via Whisper ASR and Kokoro TTS.
* **Barge-In:** Hardware audio loopback permits speaking over OctoPlus to redirect or interrupt answers instantly.
* **Spoken Proactive Nudges:** Chimes aloud when important hardware or context events occur (*"Heads up, your disk is 92% full..."*).

### 6.2 Stealth Silent Chat Mode (Libraries, Quiet Cafes, Meetings, Public Transit)
When speaking aloud is impossible or socially inappropriate, OctoPlus pivots seamlessly into a quiet, full-featured text chat interface:

1. **Instant Global Access (HUD / Spotlight Style):**
   * A single global hotkey (`Cmd + Space` on macOS, `Super + Space` on Linux) summons a minimalist floating drawer directly centered over your active workspace.
   * Immediately silences TTS audio output and pauses microphone listening to prevent accidental audio triggers or eavesdropping.
2. **Full Desktop Context Preserved (No Copy-Pasting):**
   * Even in silent text mode, OctoPlus retains full sensory vision of your desktop.
   * You can type:
     * *"Why is this compile failing?"* (Resolves *"this"* to your active IDE/terminal).
     * *"Summarize this document."* (Extracts text from your focused PDF or browser tab).
3. **Unified Thread Continuity (Voice ⇄ Text Hand-off):**
   * Conversations are persistent and cross-modal. You can start discussing a problem aloud in your car or at home, walk into a library, open the silent chat HUD, and continue the exact same dialogue without re-explaining context.
4. **Silent Streaming & Discreet Notifications:**
   * Answers stream in clean markdown with syntax-highlighted code snippets and 1-click copy buttons.
   * Proactive nudges (low battery, runaway processes, full disk) switch silently from spoken voice to subtle, interactive desktop notification banners.
5. **Mobile Silent Synchronization:**
   * When your mobile phone is set to silent or vibrate mode, the OctoPlus Mobile Link automatically defaults to an encrypted text messaging interface, allowing you to quietly interact with your laptop brain from your phone under a conference table.

---

## 7. OctoPlus Mobile Neural Link (Remote P2P Bridge)

An end-to-end encrypted remote bridge that turns your laptop into a sovereign, pocket-accessible private intelligence:

### 7.1 Cryptographic Security
* **Zero Third-Party Relays:** Direct peer-to-peer connection over the **Noise Protocol / WireGuard (Curve25519)**.
* **Air-Gapped Pairing:** One-time pairing between phone and laptop via an on-screen QR code handshake.

### 7.2 Mobile Capabilities
* **Remote Voice & Silent Chat:** Talk through your phone/earbuds when walking, or seamlessly switch to quiet text messaging in meetings.
* **Anywhere Task Dispatch:** Trigger heavy tasks remotely (*"OctoPlus, pull branch `v2-refactor`, run all unit tests, and ping me with results"*).
* **Remote Safety Confirmations:** Sensitive actions (deleting cache, sending queries to Claude) send a push notification with a 1-tap confirmation dialog.
* **Ambient Sensor Synchronization:** Streams phone GPS coordinates to the laptop so OctoPlus maintains physical environmental context on the go.

---

## 8. Target Hardware & Deployment Profile

| Component | Target Specification |
| :--- | :--- |
| **Primary Target** | **Apple MacBook Air (M4 / M5 with 24 GB Unified Memory)** |
| **Chassis Acoustics**| **100% Fanless & Silent** |
| **Battery Life Target** | **15–18 Hours Continuous Operation** |
| **Memory Allocation**| • PhaseLoopNN: ~50 MB<br>• OctoPlus Daemons + SQLite: ~400 MB<br>• Deep LLM (Standby): ~8.5 GB<br>• Free Workstation RAM: **> 14 GB** |
| **Voice Latency** | **< 300 ms to First Spoken Word** |
| **Network Default** | **Completely Offline (Air-Gapped by Kernel)** |

---

## 9. Development Roadmap

1. **Phase 1 (Foundations):** Complete OctoPlus baseline and PhaseLoopNN 3M–26M prototype.
2. **Phase 2 (Packaging & Cockpit):** Build unified installer bundle and native visual dashboard.
3. **Phase 3 (Mobile Bridge):** Implement Noise Protocol P2P socket bridge for iOS and Android.
4. **Phase 4 (Frontier Uplink & GPS):** Integrate authenticated Claude bridge and GeoClue / CoreLocation hooks.
5. **Phase 5 (Scale):** Scale PhaseLoopNN toward 1B parameter unified cognition.
