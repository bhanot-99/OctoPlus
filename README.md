# Semantic-NeuroOS

**A private laptop buddy for Linux.**

> A buddy that lives in your laptop: it notices things, remembers things, talks like a person, and nothing leaves your machine unless you say yes.

## What it is

Semantic-NeuroOS runs entirely on your own machine. It:

- **Notices** — watches your desktop and your laptop's health, and speaks first when something is worth saying: "Your disk is nearly full. Want me to show what's using the space?"
- **Remembers** — keeps a private memory of what you worked on, and of facts you ask it to remember about you.
- **Answers** — by voice or text, about "this" on your screen, about last week, or about everyday questions.
- **Looks things up, only with your OK** — it shows you the exact search text before anything is sent, and you can edit it.
- **Acts safely** — every action passes a safety gate; anything beyond the harmless needs your click on a clear dialog.
- **Knows when to be quiet** — no interruptions during calls, media, fullscreen work or quiet hours, and a simple "not now".

## The privacy promise

1. Every part that touches your data has **no network at all**, enforced by the Linux kernel.
2. One separate part can reach the internet, and only for **one request you approved**.
3. Everything that ever left your machine is listed in an **egress log** you can read.
4. Anything from the web is marked untrusted and makes later actions stricter.

## What it is not

No cloud model, no accounts, no telemetry, no auto-update, no screen capture, no stored audio, no actions behind your back.

## Status

**Phase 1 done (signed off 2026-11-16).** Every part runs as a locked-down service under `neuroos.target`, reports its health to healthd, and recovers by itself after a crash. `neuroosctl ask` streams an answer from the local model through the whole chain. The three security proofs pass against the real services. The fetcher runs as its own system user and is the only part with network access (no network code yet). Next: Phase 2, the safety kernel. Current state is always in [`memory.md`](memory.md); the phase reports are [`reports/phase-00-foundation.md`](reports/phase-00-foundation.md) and [`reports/phase-01-skeleton.md`](reports/phase-01-skeleton.md).

## Documents

| File | What it holds |
| :--- | :--- |
| [`PRD.md`](PRD.md) | What we build, for whom, and **every product number** |
| [`Architecture.md`](Architecture.md) | How it is built: parts, sockets, data, mechanisms |
| [`security.md`](security.md) | What we protect, from whom, and how each control is proven |
| [`design.md`](design.md) | Voice, wording, nudges, dialog, CLI |
| [`rules.md`](rules.md) | Binding engineering rules and the definition of done |
| [`phases.md`](phases.md) | The eleven-phase delivery plan with exit criteria |
| [`memory.md`](memory.md) | Current state — read this first every session |
| [`docs/lessons-from-v1.md`](docs/lessons-from-v1.md) | What an earlier prototype taught us |
| [`docs/adr/`](docs/adr) | Decision records (ADR-0001…0008) |
| [`BUGS.md`](BUGS.md) | Every bug, open and fixed |

## Requirements

Linux (kernel ≥ 6.7), Wayland (COSMIC first), PipeWire, systemd, a CPU with AVX2. A GPU is optional and only speeds up answers. Full details in [`PRD.md`](PRD.md) §4.
