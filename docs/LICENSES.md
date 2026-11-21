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
| **espeak-ng** | TTS phonemization library | GPL-3.0 | **Risk R-07**: Dynamically linked external library. Build instructions and sources provided. |
| **cosmic-protocols** | Wayland protocols for COSMIC | GPL-3.0-only | **Risk R-07**: Wayland protocol XML bindings. |
| **ONNX Runtime** | Inference runtime | MIT | Pinned release binary with hash verification. |
| **SQLite** | Local metadata & vector store | Public Domain | Bundled via rusqlite. |

---

## 2. Open Question OQ-06 & Legal Resolution

- **OQ-06 ("Personal install only, or public release"):** Formally resolved and closed as **Personal Use & Source-Available Self-Hosted Release**.
- **Risk R-06 Resolution:** Because deployment is scoped to self-hosted personal operation, the openWakeWord stock `hey_jarvis` model is fully compliant. Commercial redistributors must train a custom model prior to distribution.
- **Risk R-07 Resolution:** All GPL-3.0 dependencies (`espeak-ng`, `cosmic-protocols`) are isolated to their designated interfaces, dynamically linked or compiled via open build scripts, and full source code recipes are provided.

---

## 3. Dependency Policy Verification

Rust crate dependencies are audited on every release gate using `cargo-deny`:
```bash
cargo deny check licenses
```
All crates are restricted to permissive licenses (MIT, Apache-2.0, BSD-3-Clause, Unicode-3.0) or explicitly allowlisted system bindings.
