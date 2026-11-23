# Third-Party Licences & Compliance Matrix

This document records license classifications and compliance obligations for all bundled submodules, system libraries, and external machine learning models in Semantic-NeuroOS (PRD §10, phases.md §13.1).

---

## 1. Upstream Components & Model Weights

| Component | Role | Declared Licence | Compliance & Usage Notes |
| :--- | :--- | :--- | :--- |
| **llama.cpp** | Local LLM runtime (C4) | MIT | Permissive; pinned upstream submodule with copyright retained. |
| **whisper.cpp** | Speech-to-text (C2) | MIT | Permissive; pinned upstream submodule with copyright retained. |
| **Silero VAD** | Voice activity detection | MIT | Permissive ONNX model. |
| **Kokoro-82M** | Speech synthesis (TTS) | Apache-2.0 | Permissive neural voice weights. |
| **bge-small-en-v1.5** | Local text embeddings | MIT | Permissive ONNX model. |
| **openWakeWord** (stock weights) | Wake-phrase detection | Apache-2.0 (code); Non-commercial (stock weights) | **Risk R-06**: Stock pre-trained model is restricted to non-commercial / personal use. Compliant under OQ-06 Personal Use disposition. |
| **espeak-ng** | TTS phonemization library | GPL-3.0 | **Risk R-07**: dynamically linked system library (`libespeak-ng.so.1`, checked with `ldd` on 2026-11-22). Dynamic linking alone does not settle GPL obligations for the combined program. |
| **cosmic-protocols** | Wayland protocols for COSMIC | GPL-3.0-only | **Risk R-07**: a Rust crate, so it is **statically linked** into `neuroos-monitor` (no shared library; checked 2026-11-22). |
| **ONNX Runtime** | Inference runtime | MIT | Pinned release binary with hash verification. |
| **SQLite** | Local metadata & vector store | Public Domain | Bundled via rusqlite. |

---

## 2. Open Question OQ-06 & Legal Resolution

- **OQ-06 ("Personal install only, or public release"):** Formally resolved and closed as **Personal Use & Source-Available Self-Hosted Release**.
- **Risk R-06 Resolution:** Because deployment is scoped to self-hosted personal operation, the openWakeWord stock `hey_jarvis` model is fully compliant. Commercial redistributors must train a custom model prior to distribution.
- **Risk R-07 — closed (owner decision D-44, 2026-11-22):** `cosmic-protocols` (GPL-3.0-only) is compiled into `neuroos-monitor` and `espeak-ng` (GPL-3.0) is linked by `neuroos-voice`. The project is now licensed **GPL-3.0-only** (`LICENSE`, `Cargo.toml`, the `.deb` copyright file), so every binary built from it, and the `.deb`, are distributed under GPL-3.0 with their source. All other allowed dependency licences (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0; `deny.toml`) are compatible with GPL-3.0. Model weights are data with their own licences (`models/manifest.toml`). Not legal advice.

---

## 3. Dependency Policy Verification

Rust crate dependencies are audited on every release gate using `cargo-deny`:
```bash
cargo deny check licenses
```
All crates are restricted to permissive licenses (MIT, Apache-2.0, BSD-3-Clause, Unicode-3.0) or explicitly allowlisted system bindings.
