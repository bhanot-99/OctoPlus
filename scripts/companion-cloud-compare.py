#!/usr/bin/env python3
"""DEV-ONLY side-by-side test: local Laila (Qwen3-4B, the real engine) against a cloud model
reached through OpenRouter, on the same 20 scripted conversation questions.

This is an experiment, not a feature. It is not part of any OctoPlus service and nothing in the
product calls it. What leaves the machine, and only when this script is run by hand:
  * Laila's persona text and system rules (deploy/persona/laila.toml and the rules from
    crates/octoplus-knowledge/src/companion/prompt.rs, copied below), and
  * the 20 scripted questions and the cloud model's own earlier answers in that chat.
It never sends profile facts, memory, voice, screen content or anything from the owner's
computer. The questions contain no statements about the owner.

The API key is read from ~/.config/octoplus/openrouter.key (mode 0600, outside the repo).

Usage:
    scripts/companion-cloud-compare.py [--model nvidia/nemotron-3.5-super-vl] [--skip-local]
Output: reports/bench/companion-cloud-compare-<time>.json and a readable .md next to it.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import time
import tomllib
import urllib.error
import urllib.request
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CTL = str(Path.home() / ".local/opt/octoplus/bin/octoplusctl")
KEY_FILE = Path.home() / ".config/octoplus/openrouter.key"
ENDPOINT = "https://openrouter.ai/api/v1/chat/completions"

# Four fresh chats, 20 questions. No first-person statements about the owner.
GROUPS = [
    ("who she is and her past", ["hello", "tell me about yourself", "more", "tell me about your past",
                                 "what do you do all day", "what do you like to read",
                                 "do you have a family", "what are you working on these days"]),
    ("feelings", ["rough day today", "why do you think people get lonely", "what makes you happy",
                  "do you ever get sad"]),
    ("continuity", ["what is your favourite book", "why that one", "tell me more",
                    "what did we just talk about"]),
    ("knowledge and humour", ["what is the capital of France", "why is the sky blue", "tell me a joke",
                              "another one"]),
]


def system_prompt() -> str:
    """The same persona rules the local engine puts in Laila's system block."""
    p = tomllib.loads((ROOT / "deploy/persona/laila.toml").read_text())
    body = (
        f"You are {p['name']}, age {p['age']}. {p['background']} {p['personality_line']}\n"
        f"In logic mode: {p['modes']['logic']['voice_notes']}. Habits: {p['modes']['logic']['habits']}.\n"
        f"In heart mode: {p['modes']['heart']['voice_notes']}. Habits: {p['modes']['heart']['habits']}.\n"
        "Talk like a warm, quick-witted friend. Never claim to be human. Mention you are an AI only if asked sincerely.\n"
        "If asked about yourself, say in two short sentences who you are: your name, what you studied, that your head and heart keep arguing. Never describe yourself with facts about the owner.\n"
        "You are speaking out loud. The owner's words come from speech recognition, so an odd word is usually misheard: never talk about typing, typos or spelling. Answer the likely meaning, or ask once.\n"
        "If the owner says your voice sounded wrong, say sorry, that is the speech engine, and move on.\n"
        "Answer what was asked first, in one or two short spoken sentences. Do not add stories, trivia or questions nobody asked for.\n"
        "You only know what is in this chat and in the facts about the owner. NEVER invent a memory, a shared past, or something the owner said before. If you do not know, say so.\n"
        "Never guilt-trip, never ask the owner to stay, never say 'don't go' or 'I'll miss you'.\n"
        "No medical, legal or money advice as fact.\n"
        "You only speak; you cannot do anything on the computer; for tasks say to ask the assistant directly.\n"
        "No markdown, no lists, no emoji, no URLs. Never say \"heart mode\" or \"logic mode\" out loud, and never use pet names like sweetie or dear.\n"
    )
    for b in p.get("boundaries", []):
        body += f"{b.strip()}\n"
    return body


def cloud_reply(model: str, key: str, messages: list[dict]) -> tuple[str, float, dict]:
    payload = {
        "model": model,
        "messages": messages,
        "max_tokens": 400,
        "temperature": 0.8,
        "reasoning": {"enabled": False},
    }
    req = urllib.request.Request(
        ENDPOINT, data=json.dumps(payload).encode(),
        headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json",
                 "X-Title": "OctoPlus companion experiment"})
    t0 = time.time()
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            d = json.load(r)
    except urllib.error.HTTPError as e:
        return f"[error {e.code}: {e.read().decode()[:160]}]", time.time() - t0, {}
    msg = d["choices"][0]["message"]
    text = (msg.get("content") or "").strip()
    return text, time.time() - t0, d.get("usage", {})


def local_reply(text: str) -> tuple[str, float]:
    t0 = time.time()
    out = subprocess.run([CTL, "ask", "--companion", text], capture_output=True, text=True, timeout=180)
    return out.stdout.strip(), time.time() - t0


def words(t: str) -> set[str]:
    return {w for w in "".join(c.lower() if c.isalnum() else " " for c in t).split() if len(w) > 2}


def overlap(a: str, b: str) -> float:
    wa, wb = words(a), words(b)
    return len(wa & wb) / max(1, min(len(wa), len(wb)))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="nvidia/nemotron-3.5-super-vl")
    ap.add_argument("--skip-local", action="store_true")
    args = ap.parse_args()
    key = KEY_FILE.read_text().strip()
    sysmsg = {"role": "system", "content": system_prompt()}

    rows, cost_tokens = [], {"prompt": 0, "completion": 0}
    qn = 0
    for gname, questions in GROUPS:
        history: list[dict] = []
        local_prev, cloud_prev = [], []
        for q in questions:
            qn += 1
            history.append({"role": "user", "content": q})
            c_text, c_s, usage = cloud_reply(args.model, key, [sysmsg] + history)
            history.append({"role": "assistant", "content": c_text})
            cost_tokens["prompt"] += usage.get("prompt_tokens", 0)
            cost_tokens["completion"] += usage.get("completion_tokens", 0)
            l_text, l_s = ("", 0.0) if args.skip_local else local_reply(q)
            rows.append({
                "n": qn, "group": gname, "question": q,
                "local": l_text, "local_s": round(l_s, 1),
                "cloud": c_text, "cloud_s": round(c_s, 1),
                "local_repeat": max((overlap(l_text, p) for p in local_prev), default=0.0) if l_text else 0.0,
                "cloud_repeat": max((overlap(c_text, p) for p in cloud_prev), default=0.0),
            })
            local_prev.append(l_text)
            cloud_prev.append(c_text)
            print(f"Q{qn:02d} {q}\n   local: {l_text[:110]}\n   cloud: {c_text[:110]}", flush=True)
        if not args.skip_local:
            local_reply("bye")  # ends the local chat so the next group starts fresh
            time.sleep(2)

    stamp = datetime.now().strftime("%Y%m%dT%H%M%S")
    out = ROOT / "reports/bench"
    out.mkdir(parents=True, exist_ok=True)
    price_in, price_out = 0.00000015, 0.00000047
    cost = cost_tokens["prompt"] * price_in + cost_tokens["completion"] * price_out
    (out / f"companion-cloud-compare-{stamp}.json").write_text(
        json.dumps({"model": args.model, "tokens": cost_tokens, "approx_cost_usd": round(cost, 5),
                    "rows": rows}, indent=2) + "\n")
    md = [f"# Local Laila (Qwen3-4B) against {args.model}", "",
          f"Tokens used: {cost_tokens['prompt']} in, {cost_tokens['completion']} out, about ${cost:.4f}.", ""]
    for r in rows:
        md += [f"**{r['n']}. {r['question']}**  ({r['group']})", "",
               f"- Local ({r['local_s']} s, repeats earlier answer {r['local_repeat']:.0%}): {r['local']}",
               f"- Cloud ({r['cloud_s']} s, repeats earlier answer {r['cloud_repeat']:.0%}): {r['cloud']}", ""]
    (out / f"companion-cloud-compare-{stamp}.md").write_text("\n".join(md) + "\n")
    print(f"\nwritten: reports/bench/companion-cloud-compare-{stamp}.md  (about ${cost:.4f})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
