#!/usr/bin/env bash
# Tests for scripts/fetch-models.sh (P0-S06). Uses file:// sources in a temp folder;
# never touches the network (rules.md §8.4).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fetch="$root/scripts/fetch-models.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
export OCTOPLUS_FETCH_ALLOW_FILE=1
export OCTOPLUS_FETCH_HEADROOM_BYTES=0

src="$tmp/src"
mkdir -p "$src"
printf 'model-bytes-one' >"$src/one.bin"
printf 'model-bytes-two' >"$src/two.bin"
sha1="$(sha256sum "$src/one.bin" | cut -d' ' -f1)"
sha2="$(sha256sum "$src/two.bin" | cut -d' ' -f1)"
bad="$(printf '0%.0s' {1..64})"
pass=0

entry() { # id file src sha
  printf '[[model]]\nid = "%s"\nrole = "llm"\nfile = "%s"\nurl = "file://%s"\nsha256 = "%s"\nsize_bytes = 15\nlicense = "MIT"\n\n' "$@"
}

expect() { # name expected_status command...
  local name="$1" want="$2"
  shift 2
  local got=0
  "$@" >"$tmp/out" 2>&1 || got=$?
  if [[ "$want" == "ok" && "$got" -ne 0 ]] || [[ "$want" == "fail" && "$got" -eq 0 ]]; then
    echo "FAIL $name (exit $got)"
    cat "$tmp/out"
    exit 1
  fi
  pass=$((pass + 1))
}

m="$tmp/manifest.toml"
dir="$tmp/models"
{ entry one sub/one.bin "$src/one.bin" "$sha1"; entry two two.bin "$src/two.bin" "$sha2"; } >"$m"

expect "verify-only reports missing files" fail "$fetch" --manifest "$m" --models-dir "$dir" --verify-only
expect "fetch downloads and verifies" ok "$fetch" --manifest "$m" --models-dir "$dir"
[[ -f "$dir/sub/one.bin" && ! -e "$dir/sub/one.bin.part" ]] || { echo "FAIL file not in place"; exit 1; }
[[ "$(stat -c %a "$dir/two.bin")" == "444" ]] || { echo "FAIL not read-only"; exit 1; }
expect "second run verifies only" ok "$fetch" --manifest "$m" --models-dir "$dir" --verify-only
expect "single id" ok "$fetch" --manifest "$m" --models-dir "$dir" two
expect "unknown id fails" fail "$fetch" --manifest "$m" --models-dir "$dir" nope

# A tampered file on disk is fatal and not replaced.
chmod u+w "$dir/two.bin"
printf 'tampered' >"$dir/two.bin"
expect "tampered file is fatal" fail "$fetch" --manifest "$m" --models-dir "$dir"
grep -q "MISMATCH two" "$tmp/out" || { echo "FAIL no mismatch message"; exit 1; }
[[ "$(cat "$dir/two.bin")" == "tampered" ]] || { echo "FAIL tampered file was replaced"; exit 1; }

# A download whose hash does not match is deleted.
m2="$tmp/m2.toml"
entry three three.bin "$src/one.bin" "$sha2" >"$m2"
expect "bad download hash is fatal" fail "$fetch" --manifest "$m2" --models-dir "$dir"
[[ ! -e "$dir/three.bin" && ! -e "$dir/three.bin.part" ]] || { echo "FAIL bad download kept"; exit 1; }

# Manifest validation.
entry u u.bin "$src/one.bin" "" >"$m2"
expect "unpinned hash is fatal" fail "$fetch" --manifest "$m2" --models-dir "$dir"
entry u u.bin "$src/one.bin" "$bad" | sed 's/file:/http:/' >"$m2"
expect "plain http is refused" fail "$fetch" --manifest "$m2" --models-dir "$dir"
entry u ../escape.bin "$src/one.bin" "$sha1" >"$m2"
expect "path escape is refused" fail "$fetch" --manifest "$m2" --models-dir "$dir"
{ entry d d.bin "$src/one.bin" "$sha1"; entry d d2.bin "$src/one.bin" "$sha1"; } >"$m2"
expect "duplicate id is refused" fail "$fetch" --manifest "$m2" --models-dir "$dir"
{ entry x x.bin "$src/one.bin" "$sha1"; printf 'colour = "blue"\n'; } >"$m2"
expect "unknown key is refused" fail "$fetch" --manifest "$m2" --models-dir "$dir"
entry x x.bin "$src/missing.bin" "$sha1" >"$m2"
expect "failed download is reported" fail "$fetch" --manifest "$m2" --models-dir "$dir"

# file:// is refused unless the test switch is set.
entry z z.bin "$src/one.bin" "$sha1" >"$m2"
expect "file:// refused without test switch" fail env OCTOPLUS_FETCH_ALLOW_FILE=0 "$fetch" --manifest "$m2" --models-dir "$dir"

# Missing required tools fail cleanly.
expect "missing tool fails with clear message" fail env PATH="/nonexistent" /bin/bash "$fetch" --manifest "$m" --models-dir "$dir"
grep -q "fetch-models: required tool" "$tmp/out" || { echo "FAIL no required tool message"; exit 1; }

# The real manifest parses. Checked against an empty models folder so the result does not
# depend on whether the models were downloaded (hosted CI has none): every entry must be
# reported as MISSING, and nothing may fail as a manifest error.
mkdir -p "$tmp/no-models"
"$fetch" --models-dir "$tmp/no-models" --verify-only >"$tmp/real" 2>&1 || true
if grep -q "^manifest:" "$tmp/real" || ! grep -q "^MISSING" "$tmp/real" \
  || grep -vqE "^(MISSING|fetch-models: FAIL)" "$tmp/real"; then
  echo "FAIL repository manifest is valid"; cat "$tmp/real"; exit 1
fi
echo "ok   repository manifest is valid"; pass=$((pass + 1))

echo "fetch-models-test: ok - $pass checks"
