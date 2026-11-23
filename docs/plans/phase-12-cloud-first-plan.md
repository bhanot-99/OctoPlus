# Phase 12 strategy — Cloud-first answers, local model as the offline fallback, private by construction

**Status:** PROPOSAL for the owner. Nothing here is built. It changes rules R0-1/R0-2 in a controlled way, so it needs the owner's decisions in §10 and ADR-0018 (Proposed) before any code.
**Origin:** owner, 2026-11-23: "make NeuroOS completely depend on free cloud AI like Claude, agy or Perplexity, keep the local LLM only for when the laptop is offline, and be confident about privacy." Personal use only; no business goals.
**Branch:** `cloud-strategy` (docs only).

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

## 10. Decisions needed from the owner

1. **Providers:** Gemini free (P0 only), Perplexity (web facts) and Claude (quality), or fewer? Are you willing to pay a small monthly amount for the no-training providers? Set a monthly cap (suggest 5 USD to start).
2. **P1 behaviour:** show the redacted text every time, once per session, or never (trust the gate)?
3. **P2 when offline model is off:** refuse, or allow the local model to load for those questions? (Recommended: allow local.)
4. **Encryption unlock:** TPM only (no prompt), or TPM plus passphrase at login?
5. **Tags:** do you want a way to mark a note, file or topic as "never leaves"? (Recommended: yes.)
6. **Order:** start with encrypted storage (independent and useful now), or with the cloud route?

## 11. What does not change

Per-request logging in the egress ledger, C7 as the only network part, C6 as the only approver, no audio leaves, no runtime model downloads, all existing tiers and taint rules.
