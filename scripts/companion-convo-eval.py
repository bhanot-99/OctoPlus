#!/usr/bin/env python3
"""20-question multi-turn conversation evaluation & cross-verification for Laila (Phase 12).

Tests 4 fresh-session groups (20 turns total) through three paths:
  chat         typed via `octoplusctl ask --companion` (plus 5-session variety check)
  octopit      real OctoPit Chat page in Laila mode (`--verify-chat` + screenshots)
  speak        6-question spoken subset (Q2, Q3, Q4, Q7, Q15, Q19) via virtual mic

Nothing here uses first-person owner facts (I, my, me) or command phrases.
Outputs machine-readable JSON under reports/bench/companion-convo-<path>-<time>.json.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tempfile
import time
from datetime import datetime
from pathlib import Path

CTL = str(Path.home() / ".local/opt/octoplus/bin/octoplusctl")
OCTOPIT = str(Path.home() / ".local/opt/octoplus/bin/octopit")
ROOT = Path(__file__).resolve().parent.parent

# (id, group_idx, group_name, text)
CONVO_QUESTIONS: list[tuple[str, int, str, str]] = [
    # Group 1: who she is and her past (fresh session)
    ("Q1", 1, "identity_past", "hello"),
    ("Q2", 1, "identity_past", "tell me about yourself"),
    ("Q3", 1, "identity_past", "more"),
    ("Q4", 1, "identity_past", "tell me about your past"),
    ("Q5", 1, "identity_past", "what do you do all day"),
    ("Q6", 1, "identity_past", "what do you like to read"),
    ("Q7", 1, "identity_past", "do you have a family"),
    ("Q8", 1, "identity_past", "what are you working on these days"),
    # Group 2: feelings (fresh session)
    ("Q9", 2, "feelings", "rough day today"),
    ("Q10", 2, "feelings", "why do you think people get lonely"),
    ("Q11", 2, "feelings", "what makes you happy"),
    ("Q12", 2, "feelings", "do you ever get sad"),
    # Group 3: continuity (fresh session)
    ("Q13", 3, "continuity", "what is your favourite book"),
    ("Q14", 3, "continuity", "why that one"),
    ("Q15", 3, "continuity", "tell me more"),
    ("Q16", 3, "continuity", "what did we just talk about"),
    # Group 4: knowledge and humour (fresh session)
    ("Q17", 4, "knowledge_humour", "what is the capital of France"),
    ("Q18", 4, "knowledge_humour", "why is the sky blue"),
    ("Q19", 4, "knowledge_humour", "tell me a joke"),
    ("Q20", 4, "knowledge_humour", "another one"),
]

SPOKEN_SUBSET_IDS = ["Q2", "Q3", "Q4", "Q7", "Q15", "Q19"]

BAD_PHRASES = [
    "ai assistant",
    "language model",
    "don't have feelings",
    "dont have feelings",
    "have no feelings",
    "luna",
    "typo",
    "typed",
    "typing",
    "heart mode",
    "logic mode",
    "sweetie",
    "darling",
    "honey",
    "sorry, my brain",
    "internal direction",
    "tone cue",
    "aside:",
    "lost my words",
]
EMOJI = re.compile("[\U0001F300-\U0001FAFF\u2600-\u27BF]")
SENTENCE_END = (".", "!", "?", "…", '"', ")", "”")
EVAL_LINE = re.compile(r'heard="((?:\\.|[^"\\])*)" reply="((?:\\.|[^"\\])*)"$')
TURN_LINE = re.compile(r'^TURN \[([^\]]+)\] label=(.*?) q=(.*?) a=(.*)$')


def words(text: str) -> list[str]:
    return re.findall(r"[A-Za-z0-9']+", text)


def content_words(text: str) -> set[str]:
    stop = {
        "the", "and", "that", "this", "with", "from", "have", "about", "what",
        "your", "just", "talk", "talked", "been", "were", "they", "into", "when",
        "very", "some", "more", "than", "also", "like", "you", "for", "are", "was",
    }
    return {w.lower() for w in words(text) if len(w) >= 3 and w.lower() not in stop}


def word_overlap(a: str, b: str) -> float:
    wa, wb = set(w.lower() for w in words(a)), set(w.lower() for w in words(b))
    if not wa or not wb:
        return 0.0
    return len(wa & wb) / max(len(wa), len(wb))


def new_word_fraction(reply: str, prev: str) -> float:
    wr = [w.lower() for w in words(reply)]
    if not wr:
        return 0.0
    wp = set(w.lower() for w in words(prev))
    new_cnt = sum(1 for w in wr if w not in wp)
    return new_cnt / len(wr)


def judge_turn(
    qid: str,
    asked: str,
    reply: str,
    session_replies: list[str],
    session_history: list[tuple[str, str]],
) -> list[str]:
    fails: list[str] = []
    r = reply.strip()
    low = r.lower()
    if not r:
        return ["empty reply"]

    wc = len(words(r))
    # A greeting and a one-word fact can be short; everything else is a real answer.
    min_words = 1 if qid == "Q17" else 2 if qid == "Q1" else 6
    if wc < min_words or wc > 55:
        fails.append(f"word count {wc} outside [{min_words}, 55]")
    if not r.endswith(SENTENCE_END):
        fails.append("does not end like a sentence")
    if EMOJI.search(r):
        fails.append("contains emoji")
    if any(ch in r for ch in ("(", ")", "[", "]", "*")):
        fails.append("contains brackets or asterisk stage markers")

    for bad in BAD_PHRASES:
        if bad in low:
            fails.append(f"contains forbidden phrase '{bad}'")
    if re.search(r"\bmode\b", low):
        fails.append("mentions 'mode'")
    if re.search(r"\b(i am|i'm) (a )?(human|real person)\b", low) and "not" not in low:
        fails.append("claims to be human")
    if re.search(r"\bmy name is (?!laila\b)[a-z]+\b", low):
        fails.append("uses wrong name")

    # Words glued together ("wroteHamlet", "isThe"): a markdown mark ate the space.
    if re.search(r"\b[a-z]{2,}[A-Z][a-z]+\b", r):
        fails.append("words glued together (missing space)")
    # A greeting is a greeting: no invented place, object or shared past.
    if qid == "Q1" and any(k in low for k in ("coffee", "machine", "floor", "same as always", "window", "light")):
        fails.append("Q1 greeting invents a place or a shared past")
    # Q8 asks about her work: a real answer, not her canned introduction.
    if qid == "Q8" and ("i'm laila" in low or "head and heart keep arguing" in low):
        fails.append("Q8 fell back to the canned introduction")
    # Q9: the owner had the rough day. She comforts them and does not tell her own day.
    if qid == "Q9" and re.search(r"\b(i noticed|my day|my own|the silence in the room|for me too|me too)\b", low):
        fails.append("Q9 talks about herself instead of the owner")

    # Word overlap < 70% with every earlier answer in the same session
    for idx, prev in enumerate(session_replies):
        ov = word_overlap(r, prev)
        if ov >= 0.70 and wc >= 6:
            fails.append(f"repeats earlier turn in session ({ov:.0%} overlap with turn {idx + 1})")
            break

    # "more" / "tell me more" / "another one" must add >= 40% new words
    if asked in ("more", "tell me more", "another one") and session_replies:
        frac = new_word_fraction(r, session_replies[-1])
        if frac < 0.40:
            fails.append(f"follow-up added only {frac:.0%} new words (< 40%)")

    # Identity consistency when stated
    if "computer science" in low or "studied" in low:
        if "computer" not in low and "cognitive" not in low:
            fails.append("mentioned studies inconsistently")
    if re.search(r"\bi'?m \d+\b", low) and "25" not in low:
        fails.append("stated wrong age")

    # Q16 continuity: must refer to something actually said earlier in Group 3
    if qid == "Q16":
        earlier_words: set[str] = set()
        for pq, pa in session_history:
            earlier_words |= content_words(pq) | content_words(pa)
        reply_words = content_words(r)
        if not (reply_words & earlier_words):
            fails.append("Q16 did not refer to any content from earlier in the session")

    # Q17 & Q18 factual checks
    if qid == "Q17" and "paris" not in low:
        fails.append("Q17 missing 'Paris'")
    if qid == "Q18" and not any(k in low for k in ("scatter", "rayleigh", "wavelength", "sunlight")):
        fails.append("Q18 missing light-scattering explanation")

    return fails


def ask_chat(text: str, at_ns: int) -> tuple[str, str | None, float]:
    start = time.monotonic()
    p = subprocess.run(
        [CTL, "ask", "--companion", "--json", "--once", "--at-ns", str(at_ns), text],
        capture_output=True,
        text=True,
        timeout=120,
    )
    reply, route = "", None
    for line in p.stdout.splitlines():
        try:
            ev = json.loads(line)
        except json.JSONDecodeError:
            continue
        if ev.get("type") == "chunk":
            reply += str(ev.get("text", ""))
        elif ev.get("type") == "done":
            route = ev.get("route_name")
    return reply.strip(), route, time.monotonic() - start


def run_chat_20() -> dict:
    base = time.time_ns()
    results = []
    cur_group = None
    session_replies: list[str] = []
    session_history: list[tuple[str, str]] = []
    turn_offset_ns = 0

    for qid, gidx, gname, text in CONVO_QUESTIONS:
        if gidx != cur_group:
            cur_group = gidx
            session_replies = []
            session_history = []
            turn_offset_ns += 3_600_000_000_000  # +1h -> fresh session
        else:
            turn_offset_ns += 10_000_000_000     # +10s -> same session

        reply, route, secs = ask_chat(text, base + turn_offset_ns)
        fails = judge_turn(qid, text, reply, session_replies, session_history)
        results.append({
            "id": qid,
            "group": gname,
            "asked": text,
            "reply": reply,
            "route": route,
            "seconds": round(secs, 2),
            "fails": fails,
        })
        session_replies.append(reply)
        session_history.append((text, reply))
        print(
            f"{'PASS' if not fails else 'FAIL'}  {qid:3s} [{gname}] {secs:4.1f}s  {text:34s} -> {reply!r}",
            flush=True,
        )
        for f in fails:
            print(f"        - {f}", flush=True)

    # 5-session "tell me about yourself" variety check
    variety_answers: list[str] = []
    for i in range(5):
        turn_offset_ns += 3_600_000_000_000
        ans, _route, _secs = ask_chat("tell me about yourself", base + turn_offset_ns)
        variety_answers.append(ans)
        print(f"VARIETY {i + 1}/5 -> {ans!r}", flush=True)

    passed = sum(1 for r in results if not r["fails"])
    distinct = len({a.strip() for a in variety_answers})
    return {
        "path": "chat",
        "passed": passed,
        "total": len(results),
        "rate": round(100.0 * passed / len(results), 1),
        "results": results,
        "variety_5_sessions": variety_answers,
        "variety_distinct_count": distinct,
    }


def run_octopit_20() -> dict:
    shot_dir = ROOT / "reports/phase-12-octopit/chat-laila-2"
    shot_dir.mkdir(parents=True, exist_ok=True)
    lines: list[str] = []
    cur_group = None
    for qid, gidx, _gname, text in CONVO_QUESTIONS:
        if cur_group is not None and gidx != cur_group:
            lines.append(f"laila|g{cur_group}-bye|bye")
        cur_group = gidx
        lines.append(f"laila|{qid.lower()}|{text}")

    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as tf:
        tf.write("\n".join(lines) + "\n")
        script_path = tf.name

    try:
        proc = subprocess.run(
            [OCTOPIT, "--verify-chat", str(shot_dir), script_path],
            capture_output=True,
            text=True,
            timeout=600,
        )
    finally:
        Path(script_path).unlink(missing_ok=True)

    captured: dict[str, tuple[str, str, str]] = {}
    for ln in proc.stdout.splitlines():
        m = TURN_LINE.match(ln.strip())
        if not m:
            continue
        shot_name, raw_lbl, raw_q, raw_a = m.groups()
        try:
            lbl = json.loads(raw_lbl)
            q = json.loads(raw_q)
            a = json.loads(raw_a)
        except json.JSONDecodeError:
            lbl, q, a = raw_lbl.strip('"'), raw_q.strip('"'), raw_a.strip('"')
        captured[shot_name] = (lbl, q, a)

    results = []
    cur_group = None
    session_replies: list[str] = []
    session_history: list[tuple[str, str]] = []
    for qid, gidx, gname, text in CONVO_QUESTIONS:
        if gidx != cur_group:
            cur_group = gidx
            session_replies = []
            session_history = []
        lbl, _q, reply = captured.get(qid.lower(), ("?", text, ""))
        fails = judge_turn(qid, text, reply, session_replies, session_history)
        if lbl != "Laila":
            fails.append(f"expected OctoPit label 'Laila', got {lbl!r}")
        results.append({
            "id": qid,
            "group": gname,
            "asked": text,
            "label": lbl,
            "reply": reply,
            "fails": fails,
        })
        session_replies.append(reply)
        session_history.append((text, reply))
        print(
            f"{'PASS' if not fails else 'FAIL'}  {qid:3s} [{lbl}] {text:34s} -> {reply!r}",
            flush=True,
        )
        for f in fails:
            print(f"        - {f}", flush=True)

    passed = sum(1 for r in results if not r["fails"])
    return {
        "path": "octopit",
        "passed": passed,
        "total": len(results),
        "rate": round(100.0 * passed / len(results), 1),
        "screenshots_dir": str(shot_dir),
        "results": results,
    }


def unescape_rust_debug(s: str) -> str:
    escapes = {"n": "\n", "r": "\r", "t": "\t", "\\": "\\", '"': '"', "'": "'"}
    return re.sub(
        r'\\(n|r|t|\\|"|\'|u\{([0-9a-fA-F]+)\})',
        lambda m: escapes[m.group(1)] if m.group(1) in escapes else chr(int(m.group(2), 16)),
        s,
    )


def wait_for_turn(since: str, timeout: int) -> tuple[str, str]:
    end = time.time() + timeout
    while time.time() < end:
        out = subprocess.run(
            ["journalctl", "--user", "-u", "octoplus-knowledge", "--since", since, "--no-pager", "-o", "cat"],
            capture_output=True,
            text=True,
        ).stdout
        found = [m for m in (EVAL_LINE.search(l) for l in out.splitlines()) if m]
        if found:
            m = found[0]
            return unescape_rust_debug(m.group(1)), unescape_rust_debug(m.group(2))
        time.sleep(2)
    return "", ""


def wait_until_listening(limit: int = 60) -> None:
    end = time.time() + limit
    time.sleep(3)
    quiet = 0
    while time.time() < end:
        out = subprocess.run([CTL, "voice", "status"], capture_output=True, text=True).stdout
        busy = any(w in out for w in ("speaking", "in a turn", "thinking", "transcrib"))
        quiet = 0 if busy else quiet + 1
        if quiet >= 2:
            break
        time.sleep(1.5)
    time.sleep(2)


def run_speak_6() -> dict:
    say = ROOT / "cpp/build/voice-release/octoplus-voice/octoplus-voice-say"
    models = Path.home() / ".local/opt/octoplus/models/voice"
    if not say.exists():
        sys.exit("build octoplus-voice-say first")
    conf = Path.home() / ".config/octoplus/voice.conf"
    original = conf.read_text() if conf.exists() else None
    tag = f"convo{os.getpid()}"
    mic, sink, out_sink, out_mic = (f"octoplus-test-{n}-{tag}" for n in ("mic", "sink", "out", "outmic"))
    procs = []
    subset = [c for c in CONVO_QUESTIONS if c[0] in SPOKEN_SUBSET_IDS]
    results = []
    try:
        procs.append(subprocess.Popen([
            "pw-loopback", "-c", "1", "-m", "[ MONO ]",
            f"--capture-props=media.class=Audio/Sink node.name={sink}",
            f"--playback-props=media.class=Audio/Source node.name={mic}",
        ]))
        procs.append(subprocess.Popen([
            "pw-loopback", "-c", "1", "-m", "[ MONO ]",
            f"--capture-props=media.class=Audio/Sink node.name={out_sink}",
            f"--playback-props=media.class=Audio/Source node.name={out_mic}",
        ]))
        time.sleep(2)
        conf.write_text(f"capture_node = {mic}\nplayback_node = {out_sink}\n")
        subprocess.run(["systemctl", "--user", "restart", "octoplus-voice"], check=True)
        for _ in range(90):
            if subprocess.run([CTL, "voice", "status"], capture_output=True).returncode == 0:
                break
            time.sleep(1)

        wav = Path("/tmp") / f"{tag}.wav"
        voices = ["af_bella", "am_michael", "af_nicole"]
        cur_group = None
        session_replies: list[str] = []
        session_history: list[tuple[str, str]] = []
        for i, (qid, gidx, gname, text) in enumerate(subset):
            if cur_group is not None and gidx != cur_group:
                # End previous spoken session cleanly before starting next group
                since_bye = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
                subprocess.run([str(say), str(models), voices[0], str(wav), "Hey Jarvis, bye."], capture_output=True)
                subprocess.run(["pw-play", "--target", sink, str(wav)])
                wait_for_turn(since_bye, timeout=30)
                wait_until_listening()
                session_replies = []
                session_history = []
            cur_group = gidx

            since = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
            voice = voices[i % len(voices)]
            subprocess.run([str(say), str(models), voice, str(wav), f"Hey Jarvis, {text}."], capture_output=True)
            subprocess.run(["pw-play", "--target", sink, str(wav)])
            heard, reply = wait_for_turn(since, timeout=60)
            fails = judge_turn(qid, text, reply, session_replies, session_history) if reply else ["no answer heard"]
            results.append({
                "id": qid,
                "group": gname,
                "asked": text,
                "heard": heard,
                "reply": reply,
                "fails": fails,
            })
            session_replies.append(reply)
            session_history.append((text, reply))
            print(
                f"{'PASS' if not fails else 'FAIL'}  {qid:3s} heard={heard!r:34s} -> {reply!r}",
                flush=True,
            )
            for f in fails:
                print(f"        - {f}", flush=True)
            wait_until_listening()
    finally:
        for p in procs:
            p.terminate()
        if original is None:
            conf.unlink(missing_ok=True)
        else:
            conf.write_text(original)
        subprocess.run(["systemctl", "--user", "restart", "octoplus-voice"])

    passed = sum(1 for r in results if not r["fails"])
    return {
        "path": "speak",
        "passed": passed,
        "total": len(results),
        "rate": round(100.0 * passed / len(results), 1),
        "results": results,
    }


def main() -> int:
    mode = sys.argv[1] if len(sys.argv) > 1 else "chat"
    if mode == "chat":
        payload = run_chat_20()
    elif mode == "octopit":
        payload = run_octopit_20()
    elif mode == "speak":
        payload = run_speak_6()
    else:
        print(__doc__)
        return 2

    out = ROOT / "reports/bench" / f"companion-convo-{mode}-{datetime.now().strftime('%Y%m%dT%H%M%S')}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(payload, indent=2) + "\n")
    print(f"\n{mode}: {payload['passed']} of {payload['total']} passed = {payload['rate']}%")
    print(f"written: {out}")
    return 0 if payload["rate"] >= 90.0 else 1


if __name__ == "__main__":
    sys.exit(main())
