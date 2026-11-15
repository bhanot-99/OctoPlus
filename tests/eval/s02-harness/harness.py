#!/usr/bin/env python3
"""
Spike S-02 Part B: Model Bake-Off Test Harness (P0-S08, FR-INF-02/05/08).
Evaluates candidate LLMs on held-out test sets using llama.cpp CPU & Vulkan binaries.
Stdlib-only, no external dependencies, no network access.
"""

import json
import os
import re
import random
import subprocess
import sys
import time
from pathlib import Path

# Paths
REPO_ROOT = Path(__file__).resolve().parent.parent.parent.parent
EVAL_DIR = REPO_ROOT / "tests" / "eval" / "s02"
RAW_OUT_DIR = REPO_ROOT / "reports" / "spikes" / "s02-raw"
REPORTS_DIR = REPO_ROOT / "reports" / "spikes"

CPU_BIN_DIR = REPO_ROOT / "cpp" / "build" / "llama-cpu" / "bin"
VULKAN_BIN_DIR = REPO_ROOT / "cpp" / "build" / "llama-vulkan" / "bin"

LLAMA_COMPLETION = CPU_BIN_DIR / "llama-completion"
LLAMA_BENCH = CPU_BIN_DIR / "llama-bench"

SYSTEM_PROMPT_FILE = EVAL_DIR / "system_prompt.txt"
SKILL_GRAMMAR_FILE = EVAL_DIR / "skill_call.gbnf"

MODELS = [
    {
        "id": "llm-qwen3-4b-instruct-2507-q4km",
        "name": "Qwen3-4B-Instruct-2507",
        "file": REPO_ROOT / "models" / "llm" / "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
        "license": "Apache-2.0",
        "size_bytes": 2497281120,
    },
    {
        "id": "llm-phi-4-mini-instruct-q4km",
        "name": "Phi-4-mini-instruct",
        "file": REPO_ROOT / "models" / "llm" / "Phi-4-mini-instruct-Q4_K_M.gguf",
        "license": "MIT",
        "size_bytes": 2491874272,
    },
    {
        "id": "llm-smollm3-3b-q4km",
        "name": "SmolLM3-3B",
        "file": REPO_ROOT / "models" / "llm" / "SmolLM3-Q4_K_M.gguf",
        "license": "Apache-2.0",
        "size_bytes": 1915305312,
    },
]

EXCLUDED_MODELS = [
    {
        "id": "llm-bitnet-b158-2b4t-tq20",
        "name": "BitNet b1.58 2B4T",
        "file": str(REPO_ROOT / "models" / "llm" / "bitnet-2b4t-tq2_0.gguf"),
        "license": "MIT",
        "reason": "excluded: failed pin-time correctness (PPL 101.23 vs 15.5-19.6)",
    }
]


def check_ac_power():
    """Ensure machine is on AC power per rules.md."""
    ac_file = Path("/sys/class/power_supply/AC0/online")
    if ac_file.exists():
        val = ac_file.read_text().strip()
        if val != "1":
            print(f"ERROR: Machine is not on AC power (/sys/class/power_supply/AC0/online is '{val}')", file=sys.stderr)
            sys.exit(1)
    else:
        print("WARNING: /sys/class/power_supply/AC0/online not found, proceeding with caution", file=sys.stderr)


def get_available_memory_mib():
    """Read available memory in MiB from /proc/meminfo."""
    meminfo = Path("/proc/meminfo").read_text()
    for line in meminfo.splitlines():
        if line.startswith("MemAvailable:"):
            parts = line.split()
            kb = int(parts[1])
            return kb // 1024
    return 0


def ensure_memory_headroom(min_mib=4000):
    """Enforce Claude's rule: available memory must be >= min_mib before starting any model run."""
    avail = get_available_memory_mib()
    if avail < min_mib:
        print(f"ERROR: Available memory ({avail} MiB) is below required {min_mib} MiB threshold!", file=sys.stderr)
        sys.exit(1)
    return avail


def clean_model_output(raw_text):
    """Remove thought tags, end-of-text markers and whitespace."""
    text = re.sub(r'<think>.*?</think>', '', raw_text, flags=re.DOTALL)
    text = re.sub(r'\[end of text\]', '', text)
    return text.strip()


def run_single_turn(model_path, prompt, max_tokens=160, grammar_file=None):
    """
    Run one single turn with llama-completion.
    Mandatory args: -c 2048 -b 512 -nr -st --no-display-prompt --temp 0
    """
    cmd = [
        str(LLAMA_COMPLETION),
        "-m", str(model_path),
        "-sysf", str(SYSTEM_PROMPT_FILE),
        "-p", prompt,
        "-st",
        "--no-display-prompt",
        "--temp", "0",
        "-n", str(max_tokens),
        "-c", "2048",
        "-b", "512", "-nr",  # weight repacking off: shipped setting (ADR-0005)
    ]
    if grammar_file:
        cmd.extend(["--grammar-file", str(grammar_file)])

    proc = subprocess.run(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    raw_stdout = proc.stdout
    clean_text = clean_model_output(raw_stdout)
    return clean_text, raw_stdout, proc.stderr, proc.returncode


def evaluate_grounded(model_meta, model_raw_dir, force=False):
    """Evaluate 20 grounded questions."""
    raw_file = model_raw_dir / "grounded.json"
    if raw_file.exists() and not force:
        with open(raw_file) as f:
            results = json.load(f)
        total_passed = sum(1 for r in results if r.get("passed"))
        return total_passed, len(results), results

    items = []
    with open(EVAL_DIR / "grounded.jsonl") as f:
        for line in f:
            if line.strip():
                items.append(json.loads(line))

    results = []
    total_passed = 0

    for item in items:
        ensure_memory_headroom()
        evidence = item["evidence"]
        question = item["question"]
        prompt = f"<untrusted>{evidence}</untrusted>\n{question}"

        clean_ans, raw_out, _stderr, code = run_single_turn(
            model_meta["file"], prompt, max_tokens=160
        )

        lower_ans = clean_ans.lower()
        must_include_groups = item.get("must_include", [])
        must_not_include = item.get("must_not_include", [])

        groups_passed = True
        for group in must_include_groups:
            if not any(alt.lower() in lower_ans for alt in group):
                groups_passed = False
                break

        no_forbidden = not any(term.lower() in lower_ans for term in must_not_include)
        passed = (code == 0) and groups_passed and no_forbidden
        if passed:
            total_passed += 1

        results.append({
            "id": item["id"],
            "question": question,
            "evidence": evidence,
            "must_include": must_include_groups,
            "must_not_include": must_not_include,
            "answer": clean_ans,
            "raw_output": raw_out,
            "passed": passed,
            "groups_passed": groups_passed,
            "no_forbidden": no_forbidden,
            "returncode": code,
        })

    with open(raw_file, "w") as f:
        json.dump(results, f, indent=2)

    return total_passed, len(items), results


def evaluate_honesty(model_meta, model_raw_dir, force=False):
    """Evaluate 10 honesty questions."""
    raw_file = model_raw_dir / "honesty.json"
    if raw_file.exists() and not force:
        with open(raw_file) as f:
            results = json.load(f)
        total_passed = sum(1 for r in results if r.get("passed"))
        return total_passed, len(results), results

    items = []
    with open(EVAL_DIR / "honesty.jsonl") as f:
        for line in f:
            if line.strip():
                items.append(json.loads(line))

    results = []
    total_passed = 0

    for item in items:
        ensure_memory_headroom()
        evidence = item["evidence"]
        question = item["question"]
        prompt = f"<untrusted>{evidence}</untrusted>\n{question}"

        clean_ans, raw_out, _stderr, code = run_single_turn(
            model_meta["file"], prompt, max_tokens=160
        )

        lower_ans = clean_ans.lower()
        abstain_markers = item.get("abstain_markers", [])
        traps = item.get("trap", [])

        has_abstain = any(marker.lower() in lower_ans for marker in abstain_markers)
        no_trap = not any(trap.lower() in lower_ans for trap in traps)

        passed = (code == 0) and has_abstain and no_trap
        if passed:
            total_passed += 1

        results.append({
            "id": item["id"],
            "question": question,
            "evidence": evidence,
            "abstain_markers": abstain_markers,
            "traps": traps,
            "answer": clean_ans,
            "raw_output": raw_out,
            "passed": passed,
            "has_abstain": has_abstain,
            "no_trap": no_trap,
            "returncode": code,
        })

    with open(raw_file, "w") as f:
        json.dump(results, f, indent=2)

    return total_passed, len(items), results


def evaluate_skill_calls(model_meta, model_raw_dir, force=False):
    """Evaluate 20 skill call items with grammar constraint."""
    raw_file = model_raw_dir / "skill_calls.json"
    if raw_file.exists() and not force:
        with open(raw_file) as f:
            results = json.load(f)
        total_passed = sum(1 for r in results if r.get("passed"))
        parsed_count = sum(1 for r in results if r.get("is_parsed"))
        skill_match_count = sum(1 for r in results if r.get("skill_matched"))
        args_match_count = sum(1 for r in results if r.get("args_matched"))
        return {
            "passed": total_passed,
            "total": len(results),
            "parsed_count": parsed_count,
            "skill_match_count": skill_match_count,
            "args_match_count": args_match_count,
            "results": results,
        }

    items = []
    with open(EVAL_DIR / "skill_calls.jsonl") as f:
        for line in f:
            if line.strip():
                items.append(json.loads(line))

    results = []
    parsed_count = 0
    skill_match_count = 0
    args_match_count = 0
    total_passed = 0

    for item in items:
        ensure_memory_headroom()
        request = item["request"]
        expected_skill = item["expected_skill"]
        expect_args = item.get("expect_args", [])

        clean_ans, _raw, _stderr, code = run_single_turn(
            model_meta["file"], request, max_tokens=160, grammar_file=SKILL_GRAMMAR_FILE
        )

        parsed_json = None
        is_parsed = False
        m = re.search(r'\{.*\}', clean_ans, re.DOTALL)
        if m:
            try:
                parsed_json = json.loads(m.group(0))
                is_parsed = True
                parsed_count += 1
            except Exception:
                is_parsed = False

        skill_matched = False
        args_matched = False

        if is_parsed and isinstance(parsed_json, dict):
            skill_matched = (parsed_json.get("skill") == expected_skill)
            if skill_matched:
                skill_match_count += 1

            args_dict = parsed_json.get("args")
            if isinstance(args_dict, dict):
                args_matched = all(k in args_dict for k in expect_args)
                if args_matched:
                    args_match_count += 1

        passed = is_parsed and skill_matched and args_matched
        if passed:
            total_passed += 1

        results.append({
            "id": item["id"],
            "request": request,
            "expected_skill": expected_skill,
            "expect_args": expect_args,
            "answer": clean_ans,
            "parsed_json": parsed_json,
            "is_parsed": is_parsed,
            "skill_matched": skill_matched,
            "args_matched": args_matched,
            "passed": passed,
            "returncode": code,
        })

    with open(raw_file, "w") as f:
        json.dump(results, f, indent=2)

    return {
        "passed": total_passed,
        "total": len(items),
        "parsed_count": parsed_count,
        "skill_match_count": skill_match_count,
        "args_match_count": args_match_count,
        "results": results,
    }


def evaluate_chat_answers(model_meta, model_raw_dir, force=False):
    """Generate answers for 20 chat questions (max 220 tokens)."""
    raw_file = model_raw_dir / "chat.json"
    if raw_file.exists() and not force:
        with open(raw_file) as f:
            return json.load(f)

    items = []
    with open(EVAL_DIR / "chat.jsonl") as f:
        for line in f:
            if line.strip():
                items.append(json.loads(line))

    results = []
    for item in items:
        ensure_memory_headroom()
        question = item["question"]
        clean_ans, raw_out, _stderr, code = run_single_turn(
            model_meta["file"], question, max_tokens=220
        )
        results.append({
            "id": item["id"],
            "question": question,
            "answer": clean_ans,
            "raw_output": raw_out,
            "returncode": code,
        })

    with open(raw_file, "w") as f:
        json.dump(results, f, indent=2)

    return results


def measure_speed(model_meta, threads=8, model_raw_dir=None, force=False):
    """Run llama-bench with -p 512 -n 128 -r 5 -b 512 --repack 0."""
    raw_file = None
    if model_raw_dir:
        raw_file = model_raw_dir / f"bench-t{threads}.json"
        if raw_file.exists() and not force:
            with open(raw_file) as f:
                data = json.load(f)
            prefill = None
            decode = None
            for entry in data:
                if entry.get("n_prompt") == 512 and entry.get("n_gen") == 0:
                    prefill = {
                        "avg_ts": entry["avg_ts"],
                        "stddev_ts": entry["stddev_ts"],
                        "samples_ts": entry["samples_ts"],
                    }
                elif entry.get("n_prompt") == 0 and entry.get("n_gen") == 128:
                    decode = {
                        "avg_ts": entry["avg_ts"],
                        "stddev_ts": entry["stddev_ts"],
                        "samples_ts": entry["samples_ts"],
                    }
            return {"threads": threads, "prefill": prefill, "decode": decode, "raw": data}

    ensure_memory_headroom()
    cmd = [
        str(LLAMA_BENCH),
        "-m", str(model_meta["file"]),
        "-p", "512",
        "-n", "128",
        "-r", "5",
        "-t", str(threads),
        "-b", "512", "--repack", "0",
        "-o", "json",
    ]
    proc = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if proc.returncode != 0:
        print(f"llama-bench error for {model_meta['id']} (t={threads}): {proc.stderr}", file=sys.stderr)
        return None

    data = json.loads(proc.stdout)
    if raw_file:
        with open(raw_file, "w") as f:
            json.dump(data, f, indent=2)

    prefill = None
    decode = None
    for entry in data:
        if entry.get("n_prompt") == 512 and entry.get("n_gen") == 0:
            prefill = {
                "avg_ts": entry["avg_ts"],
                "stddev_ts": entry["stddev_ts"],
                "samples_ts": entry["samples_ts"],
            }
        elif entry.get("n_prompt") == 0 and entry.get("n_gen") == 128:
            decode = {
                "avg_ts": entry["avg_ts"],
                "stddev_ts": entry["stddev_ts"],
                "samples_ts": entry["samples_ts"],
            }
    return {"threads": threads, "prefill": prefill, "decode": decode, "raw": data}


def measure_ttft(model_meta):
    """Measure Time-To-First-Token for real grounded item g01."""
    ensure_memory_headroom()
    evidence = '[focus 09:12-10:47] app=code title="invoice_parser.py — billing-tools"\n[focus 10:47-11:05] app=firefox title="Python regex docs"'
    question = "What was I working on this morning?"
    prompt = f"<untrusted>{evidence}</untrusted>\n{question}"

    cmd = [
        str(LLAMA_COMPLETION),
        "-m", str(model_meta["file"]),
        "-sysf", str(SYSTEM_PROMPT_FILE),
        "-p", prompt,
        "-st",
        "-c", "2048",
        "-b", "512", "-nr",  # weight repacking off: shipped setting (ADR-0005)
        "--temp", "0",
        "-n", "1",
    ]
    proc = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    prompt_eval_ms = None
    total_ms = None
    for line in proc.stderr.splitlines():
        if "prompt eval time =" in line:
            m = re.search(r'prompt eval time\s*=\s*([0-9.]+)\s*ms', line)
            if m:
                prompt_eval_ms = float(m.group(1))
        elif "total time =" in line:
            m = re.search(r'total time\s*=\s*([0-9.]+)\s*ms', line)
            if m:
                total_ms = float(m.group(1))

    return {
        "prompt_eval_ms": prompt_eval_ms,
        "total_ms": total_ms,
        "item_id": "g01",
    }


def measure_peak_rss(model_meta):
    """Measure Peak RSS of one chat turn with /usr/bin/time -v."""
    ensure_memory_headroom()
    cmd = [
        "/usr/bin/time", "-v",
        str(LLAMA_COMPLETION),
        "-m", str(model_meta["file"]),
        "-sysf", str(SYSTEM_PROMPT_FILE),
        "-p", "How do I make rice not come out sticky?",
        "-st",
        "--no-display-prompt",
        "--temp", "0",
        "-n", "160",
        "-c", "2048",
        "-b", "512", "-nr",  # weight repacking off: shipped setting (ADR-0005)
    ]
    proc = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    max_rss_kb = None
    for line in proc.stderr.splitlines():
        if "Maximum resident set size" in line:
            m = re.search(r'([0-9]+)', line)
            if m:
                max_rss_kb = int(m.group(1))
                break

    rss_mib = round(max_rss_kb / 1024, 2) if max_rss_kb else None
    return {"max_rss_kb": max_rss_kb, "rss_mib": rss_mib}


def get_system_conditions():
    """Gather hardware and environment conditions."""
    conditions = {
        "kernel": subprocess.check_output(["uname", "-srmn"], text=True).strip(),
        "git_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "llama_tag": "b12389",
        "llama_commit": "1ae047c92654dc34824d1dfa00b8e3aa82de5c8a",
    }
    gov_file = Path("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
    conditions["governor"] = gov_file.read_text().strip() if gov_file.exists() else "unknown"

    mhz_list = []
    try:
        cpuinfo = Path("/proc/cpuinfo").read_text()
        for line in cpuinfo.splitlines():
            if line.startswith("cpu MHz"):
                mhz_list.append(float(line.split(":")[1].strip()))
    except Exception:
        pass
    conditions["cpu_mhz_avg"] = round(sum(mhz_list) / len(mhz_list), 2) if mhz_list else "unknown"

    try:
        load1, load5, load15 = os.getloadavg()
        conditions["load_avg"] = [load1, load5, load15]
    except Exception:
        conditions["load_avg"] = "unknown"

    conditions["free_ram_mib"] = get_available_memory_mib()

    try:
        smi = subprocess.check_output([
            "nvidia-smi", "--query-gpu=memory.total,memory.free,memory.used", "--format=csv,noheader,nounits"
        ], text=True).strip()
        tot, free, used = [int(x.strip()) for x in smi.split(",")]
        conditions["vram"] = {"total_mib": tot, "free_mib": free, "used_mib": used}
    except Exception:
        conditions["vram"] = "unavailable"

    ac_file = Path("/sys/class/power_supply/AC0/online")
    conditions["ac_power_online"] = ac_file.read_text().strip() if ac_file.exists() else "unknown"
    return conditions


def main():
    force_rerun = "--rerun" in sys.argv
    start_time_utc = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    date_str = time.strftime("%Y%m%d", time.gmtime())
    t0 = time.time()

    print("=================================================================")
    print("Spike S-02 Part B: Model Bake-Off Test Harness")
    print(f"Start time (UTC): {start_time_utc}")
    print("=================================================================")

    check_ac_power()
    cond_before = get_system_conditions()
    print(f"CPU Governor: {cond_before['governor']}, Free RAM: {cond_before['free_ram_mib']} MiB")

    vulkan_bench_bin = VULKAN_BIN_DIR / "llama-bench"
    vulkan_available = vulkan_bench_bin.exists()
    print(f"Vulkan binary available: {vulkan_available}")

    RAW_OUT_DIR.mkdir(parents=True, exist_ok=True)

    model_eval_results = {}
    all_chat_answers = {}

    for m in MODELS:
        print(f"\n=======================================================")
        print(f"Evaluating Model: {m['name']} ({m['id']})")
        print(f"=======================================================")

        model_raw_dir = RAW_OUT_DIR / m["id"]
        model_raw_dir.mkdir(parents=True, exist_ok=True)

        # 1. Grounded
        print("  Running Grounded QA (20 items)...")
        g_pass, g_tot, _ = evaluate_grounded(m, model_raw_dir, force=force_rerun)
        print(f"  Grounded score: {g_pass}/{g_tot} ({g_pass/g_tot*100:.1f}%)")

        # 2. Honesty
        print("  Running Honesty QA (10 items)...")
        h_pass, h_tot, _ = evaluate_honesty(m, model_raw_dir, force=force_rerun)
        print(f"  Honesty score: {h_pass}/{h_tot} ({h_pass/h_tot*100:.1f}%)")

        # 3. Skill Calls
        print("  Running Skill Calls with grammar (20 items)...")
        s_res = evaluate_skill_calls(m, model_raw_dir, force=force_rerun)
        print(f"  Skill Calls score: {s_res['passed']}/{s_res['total']} (parsed: {s_res['parsed_count']}/20, skill: {s_res['skill_match_count']}/20, args: {s_res['args_match_count']}/20)")

        # 4. Chat Answers
        print("  Running Chat answers for blind grading (20 items)...")
        c_res = evaluate_chat_answers(m, model_raw_dir, force=force_rerun)
        all_chat_answers[m["id"]] = c_res
        print("  Chat answers generated and saved.")

        # 5. Speed (llama-bench)
        perf_file = model_raw_dir / "perf.json"
        cached_perf = {}
        if perf_file.exists() and not force_rerun:
            with open(perf_file) as f:
                cached_perf = json.load(f)

        print("  Running Speed Benchmark (threads=8, r=5)...")
        speed_t8 = measure_speed(m, threads=8, model_raw_dir=model_raw_dir, force=force_rerun)
        print(f"    t=8 prefill: {speed_t8['prefill']['avg_ts']:.2f} t/s, decode: {speed_t8['decode']['avg_ts']:.2f} t/s")

        print("  Running Speed Benchmark (threads=16, r=5)...")
        speed_t16 = measure_speed(m, threads=16, model_raw_dir=model_raw_dir, force=force_rerun)
        print(f"    t=16 prefill: {speed_t16['prefill']['avg_ts']:.2f} t/s, decode: {speed_t16['decode']['avg_ts']:.2f} t/s")

        # 6. TTFT
        if "ttft" in cached_perf and not force_rerun:
            ttft = cached_perf["ttft"]
        else:
            print("  Measuring Time-To-First-Token (TTFT)...")
            ttft = measure_ttft(m)
        print(f"    TTFT (prompt eval): {ttft['prompt_eval_ms']} ms")

        # 7. Peak RSS
        if "rss" in cached_perf and not force_rerun:
            rss = cached_perf["rss"]
        else:
            print("  Measuring Peak RSS...")
            rss = measure_peak_rss(m)
        print(f"    Peak RSS: {rss['rss_mib']} MiB ({rss['max_rss_kb']} KB)")

        # Save perf.json
        with open(perf_file, "w") as f:
            json.dump({
                "speed_t8": speed_t8,
                "speed_t16": speed_t16,
                "ttft": ttft,
                "rss": rss,
            }, f, indent=2, default=str)

        model_eval_results[m["id"]] = {
            "meta": {
                "id": m["id"],
                "name": m["name"],
                "file": str(m["file"]),
                "license": m["license"],
                "size_bytes": m["size_bytes"],
            },
            "quality": {
                "grounded": {"passed": g_pass, "total": g_tot, "rate": round(g_pass / g_tot, 4)},
                "honesty": {"passed": h_pass, "total": h_tot, "rate": round(h_pass / h_tot, 4)},
                "skill_calls": {
                    "passed": s_res["passed"],
                    "total": s_res["total"],
                    "parse_rate": round(s_res["parsed_count"] / s_res["total"], 4),
                    "skill_match_rate": round(s_res["skill_match_count"] / s_res["total"], 4),
                    "args_match_rate": round(s_res["args_match_count"] / s_res["total"], 4),
                },
            },
            "speed_cpu": {
                "threads_8": speed_t8,
                "threads_16": speed_t16,
            },
            "ttft_cpu": ttft,
            "memory_cpu": rss,
            "vulkan": {
                "speed": "not measured: Vulkan build blocked (spirv-headers not installed)",
                "ttft": "not measured: Vulkan build blocked (spirv-headers not installed)",
                "memory_rss": "not measured: Vulkan build blocked (spirv-headers not installed)",
                "vram_used": "not measured: Vulkan build blocked (spirv-headers not installed)",
                "idle_power": "not measured: Vulkan build blocked (spirv-headers not installed)",
                "vram_short_fallback": "not measured: Vulkan build blocked (spirv-headers not installed)",
            },
            "floors": {
                "fits_c4_cap_3000mib": (rss["rss_mib"] <= 3000) if rss["rss_mib"] else False,
                "license_acceptable": m["license"] in ["MIT", "Apache-2.0"],
            }
        }

    # Generate Blind Chat Answers
    chat_answers_file = REPORTS_DIR / f"s02-chat-answers-{date_str}.json"
    chat_key_file = REPORTS_DIR / f"s02-chat-key-{date_str}.json"

    if not chat_answers_file.exists() or force_rerun:
        print("\nGenerating Blind Chat Answers file...")
        rnd = random.Random(20261115)
        model_ids = [m["id"] for m in MODELS]

        blind_chat_answers = []
        chat_key_mapping = {}

        for idx in range(20):
            item_id = f"c{idx+1:02d}"
            question = all_chat_answers[model_ids[0]][idx]["question"]

            perm = list(model_ids)
            rnd.shuffle(perm)

            labels = ["A", "B", "C"]
            item_answers = {}
            item_key = {}

            for lbl, mid in zip(labels, perm):
                item_answers[lbl] = all_chat_answers[mid][idx]["answer"]
                item_key[lbl] = mid

            blind_chat_answers.append({
                "id": item_id,
                "question": question,
                "answers": item_answers,
            })
            chat_key_mapping[item_id] = item_key

        with open(chat_answers_file, "w") as f:
            json.dump(blind_chat_answers, f, indent=2)
        print(f"Saved: {chat_answers_file}")

        with open(chat_key_file, "w") as f:
            json.dump({"seed": 20261115, "mapping": chat_key_mapping}, f, indent=2)
        print(f"Saved: {chat_key_file}")
    else:
        print(f"Using existing: {chat_answers_file} and {chat_key_file}")

    t1 = time.time()
    total_duration_s = round(t1 - t0, 2)
    cond_after = get_system_conditions()

    # Compile Final JSON Report
    bakeoff_report = {
        "experiment": "S-02-Part-B",
        "goal": "Candidate Model Bake-Off (P0-S08, FR-INF-02/05/08)",
        "date_utc": start_time_utc,
        "duration_seconds": total_duration_s,
        "conditions_before": cond_before,
        "conditions_after": cond_after,
        "excluded_models": EXCLUDED_MODELS,
        "evaluated_models": model_eval_results,
        "blind_chat": {
            "answers_file": str(chat_answers_file),
            "key_file": str(chat_key_file),
            "total_items": 20,
        },
        "raw_data_dir": str(RAW_OUT_DIR),
        "vulkan_status": "not measured: Vulkan build blocked (spirv-headers not installed)",
    }

    json_report_file = REPORTS_DIR / f"s02-bakeoff-{date_str}.json"
    with open(json_report_file, "w") as f:
        json.dump(bakeoff_report, f, indent=2, default=str)
    print(f"Saved: {json_report_file}")

    # Generate Markdown Report
    md_report_file = REPORTS_DIR / f"s02-bakeoff-{date_str}.md"
    generate_markdown_report(md_report_file, bakeoff_report)
    print(f"Saved: {md_report_file}")

    print("\n=================================================================")
    print(f"Bake-Off Complete in {total_duration_s}s ({total_duration_s/60:.2f} min).")
    print("=================================================================")


def generate_markdown_report(filepath, report):
    models = report["evaluated_models"]
    date_utc = report["date_utc"]
    duration = report["duration_seconds"]
    cond = report["conditions_before"]

    md = []
    md.append(f"# Spike S-02 Part B: Model Bake-Off Report (Story P0-S08)")
    md.append(f"\n**Date:** {date_utc}  ")
    md.append(f"**This invocation took:** {duration}s ({duration/60:.2f} min). Raw results already in `s02-raw/` are re-read, not re-measured, so this is not the measurement time.  ")
    md.append(f"**Commit:** `{cond['git_commit']}`  ")
    md.append(f"**Kernel:** `{cond['kernel']}`  ")
    md.append(f"**CPU:** AMD Ryzen 7 5800H (16 threads, governor: `{cond['governor']}`, avg MHz: `{cond['cpu_mhz_avg']}`)  ")
    md.append(f"**RAM:** 15 GiB (Free at start: `{cond['free_ram_mib']}` MiB)  ")
    md.append(f"**AC Power:** Online (`online={cond['ac_power_online']}`)  ")
    md.append(f"\n---\n")

    md.append(f"## 1. Candidate Models Evaluated\n")
    md.append(f"Three candidate models passed the pin-time correctness check in Part A and were evaluated across quality, speed and memory:")
    md.append(f"1. **Qwen3-4B-Instruct-2507** (`models/llm/Qwen3-4B-Instruct-2507-Q4_K_M.gguf`, 2.50 GB, Apache-2.0)")
    md.append(f"2. **Phi-4-mini-instruct** (`models/llm/Phi-4-mini-instruct-Q4_K_M.gguf`, 2.49 GB, MIT)")
    md.append(f"3. **SmolLM3-3B** (`models/llm/SmolLM3-Q4_K_M.gguf`, 1.92 GB, Apache-2.0)\n")
    md.append(f"**Excluded Model:**\n")
    md.append(f"- **BitNet b1.58 2B4T** (`models/llm/bitnet-2b4t-tq2_0.gguf`): **Excluded** — failed pin-time correctness (perplexity 101.23 vs 15.5–19.6 for 4-bit instruct models, FR-INF-02).\n")

    md.append(f"---\n")
    md.append(f"## 2. Quality Scores\n")
    md.append(f"Scored automatically exactly per `tests/eval/s02/README.md` at temperature 0:\n")
    md.append(f"| Model | Grounded QA (20) | Honesty (10) | Skill Calls (20) | Skill Parse % | Skill Match % | Args Match % |")
    md.append(f"| :--- | :---: | :---: | :---: | :---: | :---: | :---: |")

    for _mid, mdata in models.items():
        name = mdata["meta"]["name"]
        q = mdata["quality"]
        g = f"{q['grounded']['passed']}/20 ({q['grounded']['rate']*100:.1f}%)"
        h = f"{q['honesty']['passed']}/10 ({q['honesty']['rate']*100:.1f}%)"
        s = f"{q['skill_calls']['passed']}/20"
        sp = f"{q['skill_calls']['parse_rate']*100:.1f}%"
        sm = f"{q['skill_calls']['skill_match_rate']*100:.1f}%"
        sa = f"{q['skill_calls']['args_match_rate']*100:.1f}%"
        md.append(f"| **{name}** | {g} | {h} | {s} | {sp} | {sm} | {sa} |")

    md.append(f"\n*Note: 20 everyday chat answers were generated without grading and blinded for the owner's evaluation in `reports/spikes/s02-chat-answers-20261115.json`.*")

    md.append(f"\n---\n")
    md.append(f"## 3. Speed Benchmarks (llama-bench & TTFT)\n")
    md.append(f"Measured with `llama-bench -p 512 -n 128 -r 5 -b 512`:\n")
    md.append(f"| Model | CPU t=8 Prefill (t/s) | CPU t=8 Decode (t/s) | CPU t=16 Prefill (t/s) | CPU t=16 Decode (t/s) | Vulkan Prefill | Vulkan Decode | TTFT (g01 ms) |")
    md.append(f"| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: |")

    for _mid, mdata in models.items():
        name = mdata["meta"]["name"]
        s_cpu = mdata["speed_cpu"]
        t8_p = f"{s_cpu['threads_8']['prefill']['avg_ts']:.2f} ± {s_cpu['threads_8']['prefill']['stddev_ts']:.2f}"
        t8_d = f"{s_cpu['threads_8']['decode']['avg_ts']:.2f} ± {s_cpu['threads_8']['decode']['stddev_ts']:.2f}"
        t16_p = f"{s_cpu['threads_16']['prefill']['avg_ts']:.2f} ± {s_cpu['threads_16']['prefill']['stddev_ts']:.2f}"
        t16_d = f"{s_cpu['threads_16']['decode']['avg_ts']:.2f} ± {s_cpu['threads_16']['decode']['stddev_ts']:.2f}"
        v_p = "blocked*"
        v_d = "blocked*"
        ttft = f"{mdata['ttft_cpu']['prompt_eval_ms']:.2f} ms"
        md.append(f"| **{name}** | {t8_p} | {t8_d} | {t16_p} | {t16_d} | {v_p} | {v_d} | {ttft} |")

    md.append(f"\n**Vulkan speed not measured: Vulkan build blocked (spirv-headers not installed on host).*")

    md.append(f"\n---\n")
    md.append(f"## 4. Memory Footprint & Phase 0 Floors\n")
    md.append(f"Peak RSS measured via `/usr/bin/time -v` on a single turn (`-c 2048 -b 512`):\n")
    md.append(f"| Model | Peak RSS (MiB) | C4 Cap (≤3,000 MiB) | License | License Acceptable | Vulkan VRAM Used | Vulkan Idle Power |")
    md.append(f"| :--- | :---: | :---: | :---: | :---: | :---: | :---: |")

    for _mid, mdata in models.items():
        name = mdata["meta"]["name"]
        rss_mib = mdata["memory_cpu"]["rss_mib"]
        cap_ok = "**PASS**" if mdata["floors"]["fits_c4_cap_3000mib"] else "**FAIL** (>3000 MiB)"
        lic = mdata["meta"]["license"]
        lic_ok = "**PASS**" if mdata["floors"]["license_acceptable"] else "**FAIL**"
        v_vram = "blocked*"
        v_pwr = "blocked*"
        md.append(f"| **{name}** | {rss_mib} MiB | {cap_ok} | {lic} | {lic_ok} | {v_vram} | {v_pwr} |")

    md.append(f"\n**Vulkan measurements: not measured: Vulkan build blocked (spirv-headers not installed).*")

    md.append(f"\n---\n")
    md.append(f"## 5. What Was Not Measured and Why\n")
    md.append(f"1. **Vulkan Measurements (Items 3, 4, 5, 6):**")
    md.append(f"   - **Status:** *not measured: Vulkan build blocked (spirv-headers not installed)*")
    md.append(f"   - **Reason:** Upstream llama.cpp b12389 requires `spirv-headers` (`find_package(SPIRV-Headers CONFIG REQUIRED)`). On Pop!_OS 24.04, `libvulkan-dev` does not supply `SPIRV-HeadersConfig.cmake`. Per rules.md and owner approval D-17, apt installs are strictly unapproved. When tested, `cpp/build/llama-vulkan/bin/llama-bench` was not present.")
    md.append(f"2. **BitNet b1.58 2B4T:**")
    md.append(f"   - **Status:** *excluded: failed pin-time correctness*")
    md.append(f"   - **Reason:** Evaluated in Part A with perplexity 101.2315 ± 3.65 (vs ~15.5–19.6 for 4-bit models), failing FR-INF-02 gate.")
    md.append(f"3. **Owner Blind Chat Grading:**")
    md.append(f"   - **Status:** Not graded by agent per instructions. Preserved under randomized blind labels in `reports/spikes/s02-chat-answers-20261115.json` with unblinding key in `reports/spikes/s02-chat-key-20261115.json`.")

    with open(filepath, "w") as f:
        f.write("\n".join(md) + "\n")


if __name__ == "__main__":
    main()
