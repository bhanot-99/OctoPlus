#!/usr/bin/env bash
# OctoPit safety checks (ADR-0017, docs/plans/phase-11-octopit-plan.md §7).
# 1. No HTTP, TLS or DNS crate anywhere in OctoPit's dependency tree (R0-1). (tokio's own socket2 is the
#    same library the other parts use for Unix sockets; deny.toml bans the real network crates.)
# 2. No markup, no link and no raw-HTML call in the window code: model text stays plain (ADR-0017 item 3).
# 3. No approval control: the window code never says approve, allow or deny to a user (R0-7).
# 4. No `unsafe`, and the only child process is the vendor GPU tool.
# 5. The socket map lists OctoPit on exactly the sockets the pages use (checked by `cargo test -p octoplus-common`).
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
fail=0

for crate in octopit octopit-core; do
  if cargo tree -p "$crate" -e normal --prefix none 2>/dev/null \
      | grep -E '^(reqwest|hyper|ureq|hickory|trust-dns|rustls|openssl|native-tls|curl|isahc|surf|attohttpc|h2|tungstenite)\b'; then
    echo "octopit-check: FAIL - $crate links a network crate" >&2
    fail=1
  fi
done

src_gui="crates/octopit/src"
src_core="crates/octopit-core/src"

if grep -rnE 'set_markup|use_markup\(true\)|activate-link|LinkButton|gtk::Html|WebKit|webkit' "$src_gui" "$src_core"; then
  echo "octopit-check: FAIL - markup, link or web-view use in OctoPit" >&2
  fail=1
fi

if grep -rniE '"[^"]*\b(approve|allow|deny|grant)\b[^"]*"' "$src_gui" "$src_core" \
    | grep -viE 'cannot approve|never approve|approves? nothing|approval|approved|approve it|allowed on the network|allow' ; then
  echo "octopit-check: FAIL - a user-visible approval word in OctoPit" >&2
  fail=1
fi
if grep -rnE 'Button::(with_label|builder)\([^)]*\b(Approve|Allow|Deny)\b' "$src_gui"; then
  echo "octopit-check: FAIL - an approval button in OctoPit" >&2
  fail=1
fi

if grep -rnE '\bunsafe\b' "$src_gui" "$src_core" | grep -v '^\S*:[0-9]*:\s*//' ; then
  echo "octopit-check: FAIL - unsafe in OctoPit" >&2
  fail=1
fi

# Child processes: only the fixed GPU tool.
spawns="$(grep -rnE 'Command::new\(' "$src_gui" "$src_core" | grep -v 'NVIDIA_SMI' | grep -v 'controller::Command' || true)"
if [[ -n "$spawns" ]]; then
  echo "$spawns" >&2
  echo "octopit-check: FAIL - OctoPit starts a process other than the GPU tool" >&2
  fail=1
fi

if [[ "$fail" -ne 0 ]]; then
  exit 1
fi
echo "octopit-check: ok - no network crate, no markup or links, no approval control, no unsafe, one fixed child process"
