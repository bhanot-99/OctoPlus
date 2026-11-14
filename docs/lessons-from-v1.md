# Lessons from the first prototype

**Date:** 2026-11-14 · **Status:** Reference, not binding. The binding form of each lesson is the rule or design decision named beside it.

An earlier prototype of Semantic-NeuroOS reached a working voice pipeline before it was set aside. Its code was not kept; its documents are in `archive/v1-docs/`. This file keeps what it taught us so the rebuild does not pay for the same lessons twice.

Figures below are **prior evidence** from the reference machine. They are starting points to re-measure, never numbers to quote as current.

---

## 1. Process lessons

| What happened | Lesson | Now enforced by |
| :--- | :--- | :--- |
| Five phases passed their gates; a full review then found 15 high, 23 medium and 17 low bugs. No service applied its sandbox, the monitor was never connected to storage, and every health endpoint always said OK. | Passing unit tests is not "done". | rules.md §2: runs as a real sandboxed service, reports real health, checked by someone other than the builder |
| A whole phase's branch (31 commits) was never pushed and was lost with the machine. | Unpushed work does not exist. | rules.md §3.3 |
| Integration was left to a late phase. | Integrate from the first day. | phases.md Phase 1 (walking skeleton) |
| A build pipe hid a failing exit code; a disk-full build failed silently and tests ran against old binaries; the disk filled three times. | Check real exit codes; guard disk space. | rules.md §9.8, §10.1.4 |
| The state file grew to 151 KB and contradicted itself and four other documents. | One home per fact; keep the state file short. | rules.md §11 |
| An advisory was about to be ignored to get a green run. | Exceptions are the owner's decision. | rules.md §5 |

## 2. Measurement lessons

Four budget errors in one phase had the same shape: the measurement ran somewhere the shipped product does not.

| What happened | Lesson | Now enforced by |
| :--- | :--- | :--- |
| Idle CPU was measured on a silent virtual microphone, where a pre-gate skipped every frame. On real microphones it was 2–3× higher. | Measure on real devices. | rules.md §9.2 |
| A memory ceiling was set from an idle daemon; under load the peak was higher and the service would have been killed. | Measure under load. | rules.md §9.2 |
| A steady-memory figure came from a 12-minute run; the curve needed two hours to settle. | Measure for the full duration. | rules.md §9.3 |
| A coverage figure came from a build where only one target had been rebuilt. | A number from a mixed build is void. | rules.md §8.2 |
| The same binary read different CPU figures in one session because the governor and load had changed. | Record conditions with every figure. | rules.md §9.4 |
| The answer-quality score was taken with a deadline lifted, on the questions the system was tuned on, and its transcripts were in a temp folder that was later cleared. | Held-out questions, real deadlines, saved evidence. | rules.md §8.8, PRD KPI-1 |
| Voice scores used synthesised speech; the soak replayed one clip. | Real voice, real room, varied input. | rules.md §8.8 |
| A harness guard silently never fired. | Break it on purpose to prove the harness fails. | rules.md §8.9 |

## 3. Voice lessons (read before Phase 7)

| Finding | What to do |
| :--- | :--- |
| **Kokoro int8 is slower than real time** (≈ 0.9×); fp16 uses more memory than fp32 on CPU. fp32 was both fastest (≈ 5×) and smallest (≈ 429 MiB). | Start from fp32. |
| **PipeWire treats a target node as a preference.** A capture stream aimed at a missing node came up on the real microphone. | Resolve the node to an id first; if it is missing, do not capture (FR-VOI-03). |
| **A removed microphone does not look removed** by sample flow; the stream is re-linked. | Watch the registry for device changes. |
| **Whisper repeats itself** on audio with trailing silence when its audio context is shortened. | Trim silence before sizing the context. |
| **Idle CPU went to a polling loop**, not the models. | Block on audio arrival; do not poll. |
| **Memory crept up between idle and steady** because glibc kept freed large blocks in the heap. Trimming the heap after each turn fixed the unbounded part. | Trim after playback drains; expect a bounded plateau. |
| **Barge-in needs the listener running while speaking.** | Run the turn on its own thread; one consumer of the capture buffer. |
| **Streaming speech broke twice**: the two sides disagreed on a stream's name, and the requester waited for the whole answer before reading. | In the rebuild the text arrives over the request's own stream (Architecture §5.1), and tests run with the real peers. |
| **Start order hid a bug**: every harness started the model service first. | Test both start orders. |
| **Test harnesses that override `XDG_RUNTIME_DIR`** lose the audio session and leak virtual devices. | Pin the session path for audio tools in every harness. |
| **First speech chunk took ≈ 0.5 s**, mostly synthesising the first sentence. | Keep the first sentence short (design.md §9.2). |
| Prior figures: preamble ≈ 10 ms; barge-in flush ≈ 0.015 ms; transcription ≈ 96 ms; idle CPU 1.6–3.0% of one core on a real microphone; memory ≈ 447 MiB idle, ≈ 602 MiB steady, ≈ 680 MiB peak. | Budgets in PRD §6.2 are sized from these; re-measure. |

## 4. Design lessons

| What happened | Lesson | Now enforced by |
| :--- | :--- | :--- |
| A vector database brought 123,000 lines of patched third-party code, a banned network dependency, ~110 MiB, hundreds of data files, an index that went stale, and a chain of latency bugs — for a corpus of a few thousand rows. | Match the tool to the size of the problem. | Architecture §15 #1; rules.md §5 |
| A shared-memory token ring carrying ~20 tokens a second caused five bugs across two languages. | Do not optimise what is not slow. | Architecture §15 #2 |
| A graph-learning worker in a third language had 3–10 entities to learn from after eight hours of use. | Build it when the data exists. | phases.md §14 |
| A model runtime fork silently computed wrong results (perplexity 58 against 9.5). | Pin a mainstream runtime; check correctness at pin time. | PRD FR-INF-02 |
| The prefill cost of the prompt, not decoding, dominated time to first word. | Budget the prompt; cache the stable prefix. | Architecture §9 |
| One lock around the whole storage engine let a slow embedding call starve fast queries. | One writer, many readers, heavy work off the runtime. | Architecture §7.2 |
| A first query after a burst of writes was far slower than steady state. | Benchmark the cold and post-burst cases, not only the warm one. | phases.md Phase 4 PF |

## 5. Security lessons

| What happened | Lesson | Now enforced by |
| :--- | :--- | :--- |
| Taint was dropped when duplicate search results were merged, and one result type carried no taint field, which read as "clean". | Union on every merge; missing taint is an error. | rules.md R0-4; security.md §4.2 |
| Model control tokens in window titles and evidence could break the prompt structure. | Neutralise them in all outside text — local text too. | PRD FR-KNO-06 |
| Test fixtures were anonymised with an unsalted hash and committed to a public repository; the originals stayed in git history. | Keyed, discarded-key anonymiser from day one; private repository. | PRD FR-MON-12; security.md §7 |
| `PrivateNetwork` alone did not stop DNS lookups through the system resolver's socket. | Block the resolver socket as well. | Architecture §8.1 |
| An empty app id matched every process and resolved to PID 1. | Test the empty and truncated cases. | phases.md Phase 3 UT |
| "Forget" failed on a foreign-key order and left rows behind. | One transaction; verify zero rows, text hits and vectors. | PRD FR-STO-13 |
| An HTML page built from stored text allowed script injection. | Do not render outside text; C6 owns every visual surface. | Architecture §15 #8 |
| Servers died for good on one failed accept. | A bad connection costs only that connection. | rules.md AB-9 |
| No window or compositor protocol exposes a window's process id. | It is a best-effort match; treat it as a hint. | Architecture (C1 notes), spike S-05 |
