# OctoPlus — Mac SaaS Business & Launch Plan

**Date:** 2026-11-25 · **Status:** Draft plan (pre-Mac launch)  
**Product:** OctoPlus (`OctoPlus.app` + `OctoPit` UI) — macOS-only private AI companion & desktop assistant

---

## 1. Executive Summary

OctoPlus is a macOS-only AI companion and desktop assistant built around **verifiable privacy**. Unlike cloud-only AI apps that upload user context and audio to remote servers, OctoPlus runs voice, semantic memory, local embeddings, and a local model directly on the user's Mac, while routing optional cloud AI calls through a single, audited network boundary (`octoplus-fetcher`).

**Launch strategy:**
1. Finish the macOS port and core daily-use experience (Mac conversion, native `OctoPit`, Cloud/Local switch + Privacy Gate, Stealth Silent HUD).
2. Dogfood daily on the founder's MacBook for **2 months** to eliminate friction and bugs.
3. Run a small closed beta (10–50 Mac users) to refine onboarding and permissions.
4. Launch publicly as a **Mac SaaS** with a habit-forming **2-month free trial** and simple subscription pricing.

---

## 2. Target Customers & Positioning

Generic "privacy" is too broad to sell. Different customers adopt OctoPlus to solve **their specific fear**:

| Customer Segment | Core Fear / Pain | Why OctoPlus Wins | Messaging Hook |
| :--- | :--- | :--- | :--- |
| **Engineers & Developers** | Leaking company source code, `.env` secrets, or API keys into cloud AI tools. | Terminal/IDE context and debugging stay on the Mac; `Option+Space` silent HUD answers without uploading repos. | *"Debug with an AI that sees your terminal—without leaking your company's code."* |
| **Lawyers, Doctors & Consultants** | Violating client confidentiality (NDA, HIPAA, privilege rules). | Case notes, calendar, and local memory stay encrypted on disk, with an inspectable egress ledger proving what left. | *"An AI assistant that respects client privilege—with proof nothing leaked."* |
| **Everyday Employees** | IT blocking public AI tools, or accidentally pasting internal slides/emails into training data. | Remembers daily work locally and automatically redacts names, emails, and paths before any approved cloud call. | *"Remember everything you worked on, and redact sensitive details automatically."* |
| **Founders & Bosses** | Strategy, financials, payroll, or investor notes leaking—plus constant context switching. | Sensitive docs stay local (`P2`); proactive nudges and schedule awareness stay quiet during calls/presentations. | *"Your board docs never leave your MacBook, while your assistant keeps your day on track."* |
| **Solo Creators & Everyday Mac Users** | Being tracked for ads, or feeling isolated working alone at a laptop all day. | Companion mode (`Laila`) learns routines and chats naturally by voice or text, with local-only audio. | *"A smart companion inside your Mac that remembers you and doesn't spy on you."* |

**Landing page rule:** Lead with *"An AI that actually knows your Mac—without leaking your work"*, followed by tailored sections for **Developers**, **Founders**, and **Client Work**.

---

## 3. Competitive Advantage Beyond Privacy (vs. Claude Desktop & 50+ Connectors)

OctoPlus does **not** compete with Anthropic or Google by trying to build a smarter LLM—it plugs their models inside as an optional brain (Phase 13) while owning the **living macOS layer** that cloud chat apps do not build:

1. **Proactive, Not Reactive:** Cloud chat apps wait silently until opened. OctoPlus runs continuously on the Mac, notices real system/work signals (disk saturation, high CPU heat, 90-minute focus stretches, returning from idle), and speaks up or nudges at the right moment—while staying quiet during calls or fullscreen work.
2. **Automatic Timeline Memory (Zero Connector Setup):** Instead of manually linking Slack/Drive/Notion connectors or re-uploading context, OctoPlus automatically remembers your actual desktop day (focused apps, terminal spans, browser tabs, places) so *"what error was on my screen 10 minutes ago?"* works out of the box.
3. **Always-There Voice Companion + Unified Silent Thread:** Built to feel alive with hands-free wake word, open-mic sessions, fast barge-in, one consistent voice, and **one shared thread** across voice at home and the `Option+Space` silent HUD in public.
4. **Offline Reliability & Native Mac Control:** Keeps working on flights or poor Wi-Fi (local voice, local memory search, local LLM) and executes native macOS controls (volume, brightness, apps, windows, shortcuts) behind safety-gated approvals.

**Positioning line:** *Cloud AI apps give you a smart consultant in a chat window. OctoPlus gives you a proactive Mac companion that lives beside you all day—and can use Claude or Gemini as its brain whenever needed.*

---

## 4. Pricing & Trial Model

### 4.1 Pricing Tiers
| Plan | Price | Effective Monthly Rate | Notes |
| :--- | :--- | :--- | :--- |
| **Free Trial** | **$0** for **2 months** | $0 | Full local AI + capped monthly cloud AI requests |
| **Monthly** | **$20 / month** | $20.00 / mo | Matches mental anchor of ChatGPT Plus / Claude Pro |
| **6-Month** | **$90 / 6 months** | $15.00 / mo | Mid-commitment discount (25% off monthly) |
| **Annual** | **$150 / year** | $12.50 / mo | Best-value commitment (37.5% off monthly) |

### 4.2 Why the 2-Month Free Trial Works
- OctoPlus gets better over time as it builds semantic memory, place routines, and companion context.
- After 60 days, leaving means losing a personalized companion and accumulated context, driving much higher conversion than a 7- or 14-day trial.
- **Abuse safeguard:** Give **unlimited local AI** during the trial, and enforce a **monthly cloud-request cap** so heavy users cannot generate runaway API bills before subscribing.

---

## 5. Privacy-Preserving SaaS & Anti-Bypass Architecture

Charging a subscription while promising strict privacy requires clear boundaries so users trust the app:

### 5.1 Strict Network & Storage Separation
- **Storage isolation:** Only the core app/storage service can read or write user memory (`octoplus.sqlite3`, encrypted on macOS). The network module (`octoplus-fetcher`) **never** has access to user storage.
- **Minimal outbound traffic:** The network box handles only:
  1. Cloud AI requests explicitly routed/approved by the user.
  2. Minimal license/trial verification tokens.
- **Transparency:** Every outbound request (including license/trial checks) is recorded in the user-readable **egress ledger** inside `OctoPit`.
- **Offline grace period:** Signed license tokens carry a 7–14 day offline grace window so paying users are never locked out on flights or without Wi-Fi.

### 5.2 Anonymous 2-Month Trial Enforcement (No User Tracking)
To prevent infinite free-trial resets via uninstall/reinstall **without** storing user identities:
1. **Local Keychain marker:** Store trial start metadata in macOS Keychain (persists even if `OctoPlus.app` is deleted).
2. **One-way anonymous hardware hash (server-side):**
   - On first launch, the Mac hashes its hardware UUID with a one-way cryptographic hash.
   - Only the opaque hash (e.g., `9f4a7c2...`) is sent—no name, email, or personal data is stored.
   - Server stores only `hash -> trial_end_date` and issues a signed 60-day trial token. Reinstalling produces the same hash and returns the original end date.

### 5.3 Subscription Enforcement (Why Not Self-Destruct / "Auto-Cut")
- **Do NOT use binary self-destruction ("auto-cut"):** Modifying or deleting files inside `OctoPlus.app` breaks Apple's code signature, causes macOS Gatekeeper to flag the app as damaged/malware, risks revoking the Apple Developer certificate, and can lock out paying customers on transient bugs—while still failing to stop determined crackers.
- **Use server-backed value + clean locking instead:**
  1. **Apple Signing & Notarization:** Any binary tampering invalidates Apple's signature so macOS blocks modified copies by default.
  2. **Server-gated Cloud AI proxy:** API keys live only on the OctoPlus backend, never inside the Mac app. Expired/unpaid tokens are rejected cleanly by the server.
  3. **Signed license checks across core paths + updates:** Verify Ed25519-signed license tokens in core components and gate automatic app updates and new macOS skill packs behind active accounts.
  4. **Graceful lock state:** When a trial/subscription ends, show a clean upgrade screen while leaving **all local user memory intact**.

---

## 6. Cloud Model Strategy & Unit Economics

### 6.1 Why Free Developer API Tiers Don't Work for SaaS
- Free tiers (e.g., unpaid Gemini/Groq developer tiers) have tight per-minute rate limits that fail with concurrent users, forbid commercial multi-user reselling in most Terms of Service, and often permit logging/human review (incompatible with OctoPlus's privacy promise).

### 6.2 Paid Budget Tier Economics (Gemini Flash-Lite Example)
Using a paid, no-training tier such as **Gemini 3.5 Flash-Lite** (**$0.30 / 1M input tokens**, **$2.50 / 1M output tokens**; keep thinking/reasoning tokens off or low for casual turns):

*Assumed normal turn: ~800 input tokens + ~200 output tokens.*

| Usage Profile | Volume / Month | Est. Monthly API Cost / User |
| :--- | :--- | :--- |
| **Light user** | ~10 cloud turns/day (300/mo) | **~$0.22 / user / mo** |
| **Normal user** | ~20 cloud turns/day (600/mo) | **~$0.45 / user / mo** |
| **Heavy user** | ~50 cloud turns/day, large screen context | **~$2.20 / user / mo** |

**For 100 trial users over 2 months** (at ~20 turns/day):
- **Gemini 2.5 Flash-Lite** ($0.10 in / $0.40 out): **~$20 total**
- **Gemini 3.1 Flash-Lite** ($0.25 in / $1.50 out): **~$61 total**
- **Gemini 3.5 Flash-Lite** ($0.30 in / $2.50 out): **~$90 total**

At **$20/month** subscription pricing ($12.50–$15/mo on longer plans), gross margin per paying user remains very high. *(Optional: allow power users to supply their own API key for zero backend API cost.)*

---

## 7. Infrastructure, Fixed Costs & Legal Prerequisites

### 7.1 Server Hardware
Because voice, embeddings, SQLite storage, context sensing, and local inference all run on the customer's Mac, **no GPU servers are required**. The backend only validates trial hashes/subscriptions and proxies approved text to cloud AI APIs:
- **Compute:** 1 small VPS (1–2 vCPU, 1–2 GB RAM, ~$5–$10/month) or serverless workers.
- **Database:** Lightweight Postgres or SQLite for `trial_hash -> end_date` and subscription records.
- **Billing:** Stripe, Paddle, or LemonSqueezy (handles cards, subscriptions, and global tax).
- **Apple Developer Program:** **$99/year** (required for Developer ID signing & notarization of `OctoPlus.app`).

### 7.2 Licensing Prerequisite (Critical Before Commercial Launch)
- Currently, the Linux repository is licensed `GPL-3.0-only` due to Linux dependencies (`cosmic-protocols` and `espeak-ng`).
- When completing the macOS-only conversion (Phase 15) and removing Linux code, any remaining GPL-licensed dependencies (e.g., `espeak-ng` if bundled) must be replaced with permissive alternatives (MIT / Apache-2.0 / BSD) before distributing proprietary commercial builds.

---

## 8. Practical Go-To-Market Sequence

1. **Build the core Mac product first (before full 20-phase scope):** Focus on Phase 15 (Mac port) + Phase 13B (Secure Enclave + Cloud/Local switch) + Phase 16 (SwiftUI `OctoPit`) + Phase 17 (Silent HUD). Keep advanced skill packs and agent mesh modular so they don't delay initial launch.
2. **2-Month Founder Dogfooding:** Use OctoPlus daily on MacBook Air; fix onboarding friction, TCC permission prompts, battery/thermal throttling, and latency.
3. **Closed Beta (10–50 Mac Users):** Give free access, watch setup drop-offs, and ensure first-run setup reaches a working answer in under 2 minutes.
4. **Public Launch Channels:**
   - Short 30-second screen demos showing *"Option+Space screen context + local proof"* on X/Twitter and `r/macapps`.
   - Technical deep-dive post on **Hacker News** showcasing the Rust/C++/Swift zero-egress architecture and inspectable egress ledger.
5. **Revenue Milestone Targets (Bootstrapped Indie Scale):**
   - 100 paid users @ ~$15 avg/mo = **~$1,500 MRR** (covers all tools, APIs, and living costs in early stage)
   - 500 paid users @ ~$15 avg/mo = **~$7,500 MRR**
   - 2,000 paid users @ ~$15 avg/mo = **~$30,000 MRR**
