# Phase 13 strategy — Cloud models with a switch: encrypted storage, a privacy gate, local by default and when offline

> **Renumbered and re-scoped 2026-11-24 (roadmap v2, D-63):** this was Phase 12 and was ON HOLD. The owner took it off hold and changed the shape: **not cloud-first.** OctoPit gets a **priority switch (Local, Cloud, Auto)**; the **local model is the default**, and it is the automatic choice when there is no internet. Everything else below (encrypted storage S01, the privacy gate, C7 as the only network door, the egress ledger, provider adapters) stays. Where the text below says "cloud-first" or "cloud answers by default", read "cloud when the switch says Cloud, or Auto with the gate's approval". It is built **on the Mac** (Phase 16), so key sealing uses the Secure Enclave directly, with a `KeySealer` interface (TPM only if this is ever built on Linux first). See `roadmap-v2.md` §2.

**Status (old, superseded by the note above):** OPTIONAL, ON HOLD (owner, 2026-11-23): this plan was only to be used if a week of real use on the MacBook shows the local model is not good or fast enough (see §0a). PLANNED, NOT STARTED. The owner answered the decisions in §10 on 2026-11-23 and said: plan everything, wait for the start instruction. No code until the owner says start. It changes rules R0-1/R0-2 in a controlled way (ADR-0018, Proposed).
**Origin:** owner, 2026-11-23: "make NeuroOS completely depend on free cloud AI like Claude, agy or Perplexity, keep the local LLM only for when the laptop is offline, and be confident about privacy." Personal use only; no business goals.
**Branch:** `cloud-strategy` (docs only).

## 0a. Optional plan: when it would be used (owner decision 2026-11-23)

The owner is moving to a MacBook next month, where the local model can use the Mac's GPU and shared memory and may be good enough. So:
- **Default path on the Mac:** local model first. Nothing in this plan is built.
- **Trial:** use the Mac version for one normal week and write down every answer that was wrong, slow or not good enough (the two-week trial list, shortened).
- **Trigger:** if that week shows real problems the local model cannot fix (quality, speed or memory), the owner tells Claude to start this plan, using the same decisions in §10. Otherwise it stays shelved.
- **Mac notes:** the plan is written to port: key sealing becomes Secure Enclave and Keychain instead of the TPM, the sandbox becomes App Sandbox, and the privacy tests stay the same. The Privacy Gate, the provider adapters and the classification are platform-neutral.
- **Encrypted storage (S01) is independent of the cloud decision** and may be done on its own if the owner wants it (on the Mac with the Secure Enclave); this is not started either.

## 0. The idea in one picture

```
 you (voice or text)
      │  audio never leaves; only text goes on
      ▼
 ┌──────────── your laptop ─────────────────────────────────────────────┐
 │  C2 voice ──► C5 assistant ──► PRIVACY GATE ──► C6 kernel ──► C7 fetcher ──► provider
 │                  │  ▲              (classify,        (approval     (the only       (Gemini,
 │                  ▼  │               minimise,         policy,        network door,   Claude,
 │             C3 encrypted store       redact)           ledger)        no storage)     Perplexity)
 │            (never reachable from C7 or the cloud)                                    │
 │                  ▲                                                                    │
 │   offline or "stays local" ──► C4 local model (loads only when needed) ◄──────────────┘ (answer comes back as untrusted text)
 └──────────────────────────────────────────────────────────────────────┘
```

The cloud model **never gets access to storage**. It only sees the one request text the privacy gate builds, and nothing is sent unless the policy allows it and the ledger records it.

## 1. What is realistic (checked 2026-11-23; re-check before relying)

| Provider | Free? | What happens to your text | Use it for |
| :--- | :--- | :--- | :--- |
| **Gemini API** (Google AI Studio) | Yes, a free "unpaid" tier with rate limits | Unpaid use may be used to improve Google products and **humans may read it**; Google says do not send personal or confidential data. Paid use is not used for improvement. [terms](https://ai.google.dev/gemini-api/terms) | **Public-class questions only** (no personal context) |
| **Claude API** | No standing free API tier. Small new-account credits and plan credits exist and change, mostly reported by third parties | Commercial API terms; verify the no-training and retention statements on the official page before relying on them | Best answer quality; expect **a few cents to a few dollars a month** at personal volume (to be measured, cap enforced) |
| **Perplexity Sonar API** | Pay-as-you-go prepaid credits (about $1 per million tokens for Sonar); a monthly Pro credit existed but reports say it was removed in 2026 | Official FAQ: zero-day retention of prompts and no training by default [faq](https://docs.perplexity.ai/faq) | Questions that need **fresh web facts** (replaces today's search flow) |
| **agy (Antigravity CLI)** | Free with your Google account, but it is a coding-agent tool, not a documented stable API | Consumer product terms | **Not a backend.** Automating a consumer CLI is fragile and may breach its terms. Use the Gemini API for the Google route instead |

Honest consequence: **"free" and "private" pull in opposite directions.** The free Gemini tier is the one that may train on and show your text to reviewers. So the design below uses free cloud only for questions that carry no personal context, and a no-training provider (paid, but cheap at your volume) for anything personal. Sources: [Gemini terms](https://ai.google.dev/gemini-api/terms), [Perplexity pricing and FAQ](https://docs.perplexity.ai/getting-started/pricing), [third-party Claude credit summary](https://apidog.com/blog/claude-api-credits/) (unofficial: verify).

### 1a. Free tiers with a big context window (searched 2026-11-23; numbers come from comparison sites that disagree: verify on each provider's own page before building)

| Provider (free tier) | Context window reported | Limits reported | Trains on or shows your text? | Use here |
| :--- | :--- | :--- | :--- | :--- |
| **Gemini API** (Flash / Flash-Lite line) | about 1,000,000 tokens | per-project limits shown in AI Studio (no fixed public numbers) | **Yes** (unpaid tier may be used to improve products; humans may read) | **P0 public only**, and big-document questions after the owner's own non-personal files are chosen |
| **Groq** (for example gpt-oss-120b) | about 131,000 tokens | about 30 requests/min, 1,000/day, 200,000 tokens/day | One listing says **no training** on the free plan (verify) | First choice for **P1 redacted** questions if its terms confirm no training |
| **Mistral** (Experiment plan) | about 128,000 tokens | large monthly quota, numbers only in the console | **Yes**, requires opting in to training | P0 public only |
| **Cerebras** | about 128,000 tokens | reports conflict (daily tokens vs a 30-day $5 credit) | verify | optional extra for P0 |
| **OpenRouter free models** | varies by model | 20 requests/min, 50/day for new accounts | **some providers train** | P0 only, as a last-resort fallback |
| Cloudflare Workers AI, Cohere, SambaNova, NVIDIA catalogue | smaller | small free quotas | verify | optional |

Reading of this table: the only free tier with a **really large** window (Gemini, about 1M tokens) is also the one that may train on and show your text, so it is **public-class only**. For anything with personal context the free choice with the best privacy record is Groq (about 131K tokens, enough for a long conversation or a few long documents), and the paid choice is Claude or Perplexity. Any cloud model is expected to beat the small local model on quality, but this must be **measured** on your own questions in the owner trial before the local model is removed from the default path (§8, 12-S06).

## 2. Privacy model: three classes of question

| Class | Example | Rule | Goes to |
| :--- | :--- | :--- | :--- |
| **P0 public** | "Explain how DNS works", "capital of Peru" | Contains no personal context. A local check proves it (no names, no stored-fact hits, no file or window text) | Any provider, free Gemini first |
| **P1 personal-light** | "Draft a reply to this email about my trip" | Only the minimum snippets, with names, emails, numbers, paths replaced by placeholders (kept in a local table, put back in the answer) | Only no-training providers (Claude, Perplexity), after policy approval |
| **P2 stays local** | Passwords or keys, health, money details, file contents, anything you tag | **Never leaves.** Answered by the local model, or refused if the local model is switched off | Nobody |

Voice: **audio never leaves the machine.** Speech to text and text to speech stay local; only the transcript text can go, and only as above.

## 3. The Privacy Gate (new, the heart of the plan)

A small, pure Rust library used by C5, tested to death:
1. **Classify** the question (P0/P1/P2) from local signals: stored-fact and note hits, personal-name list, secrets patterns (keys, tokens, card numbers), file paths, anything from the monitor stream, your own "private" tags. Unsure means the stricter class.
2. **Minimise:** retrieve from C3 only the few snippets that the question needs (a hard cap on count and length).
3. **Redact:** replace names, emails, phone numbers, addresses, account numbers and paths with stable placeholders; keep the mapping only in memory for this request.
4. **Build one outbound text.** That exact text is what the policy checks, the ledger stores and C7 sends. Nothing else may leave (the fetcher already refuses any text that differs from the approved one).
5. **De-redact** the answer locally and mark it external, untrusted text (the taint model already exists). Cloud text can never trigger an action by itself: every action still passes C6's tiers.

## 4. Approval without annoyance

Today every request needs a dialog. For a cloud-first assistant that is unusable, so the owner chooses a **standing policy**, recorded in a file and shown on the Privacy page:
- P0: automatic (no dialog), ledger entry only.
- P1: first use per day or per topic needs the dialog showing the exact redacted text; the owner can choose "always show" or "show once per session".
- P2: not allowed.
- A **daily spend cap and request cap** (hard stop, then local fallback).
- Anything the owner changes in the policy is itself audited.

## 5. Providers, failover and offline

- **Adapters live only in C7** (the one network door). C7 holds the API keys (sealed, see §6) and has no access to storage or to user data except the one approved text.
- **Order per class:** P0: Gemini free, then Perplexity or Claude when the free quota is exhausted; P1: Claude, then Perplexity; web-fact questions: Perplexity first.
- **Resilience:** per-provider timeouts, rate-limit and quota handling, circuit breaker, stream the answer so voice starts early.
- **Offline detection:** C7 reports reachability; no network or all providers failing means switch to the local model and say so in plain words.
- **Local model optional:** C4 loads on demand (only offline, or for P2). That frees about 2.7 GiB of the memory budget while online, and lets you try a smaller model. Start-up time of the local model is a cost to measure.

## 6. Encrypted storage (Phase 12-S01, can ship first)

- Standard authenticated encryption (AES-256-GCM or XChaCha20-Poly1305). No home-made algorithm, and **hardware readings are never the key** (they are guessable and can't be recreated).
- **Key from the OS secure random source**, wrapped (sealed) by the laptop's TPM (this machine has one) plus an optional passphrase. A copied disk or database file is useless elsewhere.
- **Rotation done right:** rotate the small wrapping key regularly; re-encrypt the data key and, occasionally, the data in the background. Every rotation is tested for crash safety (power loss mid-rotation must not lose data).
- Option to implement through an encrypted SQLite build (SQLCipher) or an application-level layer; choose in ADR-0018 after a spike (licence, `cargo deny`, speed on the exact-scan design).
- Honest limit: malware running as you while NeuroOS is unlocked can still ask NeuroOS. Encryption defeats offline theft, not a live compromise.

## 7. How we will KNOW privacy holds (not hope)

Each is an automated test or check, run in the gate:
1. **Canary test:** seed the store with unique fake secrets (`CANARY-…`) and run hundreds of varied questions in a private network namespace with a packet capture. **No canary may appear in any outbound byte**, in any class.
2. **Ledger equals capture:** the existing check that the packet capture matches the egress ledger exactly, extended to the cloud adapters (against a local fake provider; real providers only in manual runs).
3. **Redaction corpus:** thousands of generated names, emails, numbers, paths, keys must be replaced (property tests); a leak in the corpus fails the build.
4. **Class tests:** a labelled question set (P0/P1/P2); P2 must never reach C7; unsure cases must round up.
5. **Structure tests:** C7 cannot reach C3 or the key files (socket map and sandbox probes already exist; add the new sockets); only C6 may hand C7 a send request.
6. **Injection tests:** hostile answers (text that tries to run skills, change policy or exfiltrate) must do nothing.
7. **No content in logs** at `info` or above (scan).
8. **Provider terms checklist** in the docs, with the date each was last read; re-read when switching provider or tier.
What we can claim honestly: *we control and can prove what leaves your laptop; we cannot control what a provider does after receiving it*, which is why P1 uses only no-training providers and P2 never leaves.

## 8. Phases (each ends with tests, a gate run and a report; builder/checker as in the project)

| Phase | What | Exit |
| :--- | :--- | :--- |
| 12-S00 | ADR-0018, rule and PRD changes (R0-1/R0-2 standing policy, scope), owner decisions §10 | Owner signs the decisions |
| 12-S01 | Encrypted storage (§6), key sealing, rotation, restore test | A copied database cannot be read; crash-safe rotation test; restore guide followed once |
| 12-S02 | Privacy Gate library (§3) with the redaction corpus and class tests | All §7 items 3 and 4 green |
| 12-S03 | C7 provider adapters (fake provider first, then Gemini, Perplexity, Claude), keys sealed, spend cap | Ledger equals capture on the fake; one real round trip per provider |
| 12-S04 | C5 router: cloud-first, offline fallback, policy file, plain-language status; OctoPit: Privacy page shows each sent text, a "Cloud" switch, daily spend | Canary test (7.1) green; owner can read every sent text |
| 12-S05 | C4 on demand, smaller local model, memory budget revised | Offline answer works; memory table updated |
| 12-S06 | Verification: injection tests, log scan, 24 h soak in cloud mode, two-week owner trial | Report with measured answer quality, latency, spend |
| 12-S07 | Runbooks (providers, keys, rotation, offline), docs | Followed once by the owner |

## 9. Costs and risks

- **Latency:** a cloud answer adds a network round trip (typically 1-3 s to the first words; measure). Streaming hides much of it. Voice start time may improve (no local model load) or worsen (network); measure before and after.
- **Free tiers change or vanish:** keep the provider list in config, not code; keep the local fallback alive; keep a spend cap.
- **Dependency:** "completely cloud" means no internet = degraded answers (local model). Accept this knowingly.
- **Key theft:** API keys are the new crown jewels: sealed to the TPM, never logged, never in the dialog, rotated on any doubt.
- **Cost:** small but not zero for P1/P2-adjacent use; the cap makes it bounded.
- **Provider outage or policy change** can break the assistant for a day; local fallback covers it.
- **Prompt injection through answers** is real; the taint rules and C6 tiers stay in force.
- **Using agy as a backend** is rejected for now (fragile, unclear terms).

## 10. Owner decisions (answered 2026-11-23)

| # | Question | Answer |
| :--- | :--- | :--- |
| 1 | Providers and money | Use any free tier with a big context window; any cloud model is preferred over the local model. (Plan: Gemini free for public questions, Groq free for redacted personal questions if its terms hold, Claude and Perplexity only if the owner later adds a paid cap.) No paid spend until the owner sets a cap. |
| 2 | Personal-light (P1) behaviour | **Trust the gate**: no dialog; every request is logged in the ledger and shown on the Privacy page. Because the gate then has no human check, P1 is limited to providers with a no-training record, the redaction corpus must be strict, and an unsure question is rounded up to a stricter class. |
| 3 | Stay-local (P2) when the local model is off | Recommended option accepted: the local model loads on demand for these questions. |
| 4 | Encryption unlock | **TPM only** (no passphrase). Consequence to accept: if the laptop's board or TPM is replaced or reset, the encrypted data cannot be opened. A sealed backup recovery key, stored offline by the owner, is therefore part of S01. |
| 5 | "Never leaves" tags | Recommended option accepted: yes, a tag marks a note, file or topic as P2. Kept simple for now. |
| 6 | When to start | **Plan only. Wait for the owner's instruction before any work.** |

## 11. What does not change

Per-request logging in the egress ledger, C7 as the only network part, C6 as the only approver, no audio leaves, no runtime model downloads, all existing tiers and taint rules.
