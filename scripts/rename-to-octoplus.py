#!/usr/bin/env python3
"""One-time tool for Phase 11.5: rename Semantic NeuroOS to OctoPlus (P115-S02).

    scripts/rename-to-octoplus.py              dry run: prints what it WOULD do, changes nothing (default)
    scripts/rename-to-octoplus.py --apply      rewrite file contents, then `git mv` the paths

Rules and the list of things that must not change: reports/rename-inventory.md (sections 3, 6 and 8).
Safe to run twice: the second run finds nothing to do. This file and the migration script keep the old
name on purpose, so they are never rewritten.
"""
import argparse
import collections
import os
import re
import subprocess
import sys

# Never rewritten: history, evidence, plans that describe the rename, generated files, binary fixtures,
# and the two tools that must keep the old name.
SKIP_PREFIXES = ("reports/", "docs/adr/", "docs/design/patches/", "docs/plans/")
SKIP_FILES = {"memory.md", "scripts/rename-to-octoplus.py", "scripts/migrate-from-neuroos.sh"}
SKIP_SUFFIXES = (".lock", ".dump")  # lock files are regenerated with cargo; dumps with the project's generator

# The GitHub address keeps its old name until the owner renames the repository (inventory decision 13).
URL_KEEP = re.compile(r"github\.com/bhanot-99/Semantic-NeuroOS")
SENTINEL = "\x00GHURL\x00"

# Ordered, longest first. (name, pattern, replacement)
RULES = [
    ("octopit-core (crate)", re.compile(r"neuroos-octopit-core"), "octopit-core"),
    ("octopit_core (Rust path)", re.compile(r"neuroos_octopit_core"), "octopit_core"),
    ("octopit (binary and crate)", re.compile(r"neuroos-octopit"), "octopit"),
    ("octopit (Rust path)", re.compile(r"neuroos_octopit"), "octopit"),
    ("octoplusctl", re.compile(r"neuroosctl"), "octoplusctl"),
    ("liboctoplus", re.compile(r"libneuroos"), "liboctoplus"),
    ("package name semantic-neuroos", re.compile(r"semantic-neuroos"), "octoplus"),
    ("Semantic NeuroOS (prose)", re.compile(r"Semantic[ -][Nn]euro[Oo][Ss]"), "OctoPlus"),
    ("NEUROOS (constants)", re.compile(r"NEUROOS"), "OCTOPLUS"),
    ("NeuroOS (prose)", re.compile(r"NeuroOS"), "OctoPlus"),
    ("neuroos (identifiers, paths)", re.compile(r"neuroos"), "octoplus"),
]
ARTICLE = re.compile(r"\b([Aa]) (?=OctoPlus)")  # "a NeuroOS" became "a OctoPlus"; fix the grammar


def tracked_files():
    out = subprocess.run(["git", "ls-files", "-z"], capture_output=True, check=True).stdout
    return [p for p in out.decode().split("\0") if p]


def skipped(path):
    return path in SKIP_FILES or path.startswith(SKIP_PREFIXES) or path.endswith(SKIP_SUFFIXES)


def rewrite_text(text, counts=None):
    text = URL_KEEP.sub(SENTINEL, text)
    for name, pat, rep in RULES:
        text, n = pat.subn(rep, text)
        if n and counts is not None:
            counts[name] += n
    text, n = ARTICLE.subn(lambda m: m.group(1) + "n ", text)
    if n and counts is not None:
        counts["grammar: a to an"] += n
    return text.replace(SENTINEL, "github.com/bhanot-99/Semantic-NeuroOS")


def rewrite_path(path):
    p = URL_KEEP.sub(SENTINEL, path)
    for _, pat, rep in RULES:
        p = pat.sub(rep, p)
    return p.replace(SENTINEL, "github.com/bhanot-99/Semantic-NeuroOS")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--apply", action="store_true", help="really change files (default is a dry run)")
    args = ap.parse_args()
    root = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True).stdout.strip()
    os.chdir(root)

    counts = collections.Counter()
    per_folder = collections.Counter()
    changed, unreadable, symlinks = [], [], []
    for path in tracked_files():
        if skipped(path) or not os.path.isfile(path):
            continue
        if os.path.islink(path):
            symlinks.append(path)
            continue
        try:
            with open(path, encoding="utf-8", newline="") as fh:
                text = fh.read()
        except UnicodeDecodeError:
            if "neuroos" in path.lower():
                unreadable.append(path)
            continue
        new = rewrite_text(text, counts)
        if new != text:
            changed.append((path, new))
            per_folder["/".join(path.split("/")[:2]) if "/" in path else "(top level)"] += 1

    # Path moves
    moves, collisions, seen = [], [], set()
    for path in tracked_files():
        if skipped(path):
            continue
        dest = rewrite_path(path)
        if dest != path:
            if dest in seen or (os.path.exists(dest) and dest != path):
                collisions.append((path, dest))
            seen.add(dest)
            moves.append((path, dest))

    print(f"mode: {'APPLY' if args.apply else 'DRY RUN (nothing is changed)'}")
    print(f"files whose contents change: {len(changed)}")
    print("replacements by rule:")
    for name, _, _ in RULES:
        if counts[name]:
            print(f"  {counts[name]:6d}  {name}")
    if counts["grammar: a to an"]:
        print(f"  {counts['grammar: a to an']:6d}  grammar: a to an")
    print(f"total replacements: {sum(counts.values())}")
    print("files changed per folder:")
    for k, v in per_folder.most_common(14):
        print(f"  {v:5d}  {k}")
    print(f"paths to move with git mv: {len(moves)}")
    print(f"collisions: {len(collisions)}")
    for a, b in collisions[:10]:
        print(f"  COLLISION {a} -> {b}")
    if unreadable:
        print(f"binary files with the name in their path (moved, contents untouched): {len(unreadable)}")
    if symlinks:
        print(f"tracked symlinks (paths only): {len(symlinks)}")

    if collisions:
        print("refusing to continue: fix the collisions first", file=sys.stderr)
        return 2
    if not args.apply:
        return 0

    for path, new in changed:
        with open(path, "w", encoding="utf-8", newline="") as fh:
            fh.write(new)
    # Move longest paths first so a folder is never moved before the files inside it are named.
    for src, dst in sorted(moves, key=lambda m: -len(m[0])):
        if not os.path.exists(src):
            continue  # already moved with its parent
        os.makedirs(os.path.dirname(dst) or ".", exist_ok=True)
        subprocess.run(["git", "mv", src, dst], check=True)
    print("applied. Next: regenerate lock files and generated units, rebuild, run the tests.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
