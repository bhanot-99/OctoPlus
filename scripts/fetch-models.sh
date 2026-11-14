#!/usr/bin/env bash
# Fetch and verify every model in models/manifest.toml (P0-S06, NFR-SEC-05, R0-9).
#
#   fetch-models.sh [--verify-only] [--models-dir DIR] [--manifest FILE] [ID ...]
#
# * Each file is downloaded over HTTPS to <file>.part, its SHA-256 checked, and only
#   then renamed into place. A mismatch deletes the .part file and fails.
# * An existing file with a wrong hash is never trusted or replaced silently: it fails.
# * --verify-only checks what is present and downloads nothing.
# * Exit status is non-zero if any entry is missing, unpinned or wrong.
#
# Python's tomllib is used only to read the manifest; downloads use curl.
set -euo pipefail

for tool in python3 curl sha256sum; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "fetch-models: required tool '$tool' not found in PATH" >&2
    exit 1
  fi
done

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$root/models/manifest.toml"
models_dir="${NEUROOS_MODELS_DIR:-$root/models}"
verify_only=0
ids=()
# Tests set this to allow file:// sources; never set it for real downloads.
protocols="=https"
if [[ "${NEUROOS_FETCH_ALLOW_FILE:-0}" == "1" ]]; then
  protocols="=https,file"
fi

while (($#)); do
  case "$1" in
    --verify-only) verify_only=1 ;;
    --models-dir) models_dir="$2"; shift ;;
    --manifest) manifest="$2"; shift ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    -*) echo "fetch-models: unknown option $1" >&2; exit 2 ;;
    *) ids+=("$1") ;;
  esac
  shift
done

# Emit one tab-separated line per entry: id file url sha256 size license.
read_manifest() {
  python3 -I - "$manifest" <<'PY'
import re, sys, tomllib
with open(sys.argv[1], "rb") as f:
    data = tomllib.load(f)
allowed = {"model"}
extra = set(data) - allowed
if extra:
    sys.exit(f"manifest: unknown top-level keys {sorted(extra)}")
need = ["id", "role", "file", "url", "sha256", "size_bytes", "license"]
roles = {"llm", "embedding", "stt", "tts", "wake", "vad"}
seen = set()
for m in data.get("model", []):
    missing = [k for k in need if k not in m]
    unknown = set(m) - set(need)
    if missing or unknown:
        sys.exit(f"manifest: entry {m.get('id', '?')}: missing {missing} unknown {sorted(unknown)}")
    if m["id"] in seen:
        sys.exit(f"manifest: duplicate id {m['id']}")
    seen.add(m["id"])
    if m["role"] not in roles:
        sys.exit(f"manifest: {m['id']}: bad role {m['role']}")
    if not re.fullmatch(r"[0-9a-f]{64}", m["sha256"]):
        sys.exit(f"manifest: {m['id']}: sha256 is not pinned")
    if not m["url"].startswith(("https://", "file://")):
        sys.exit(f"manifest: {m['id']}: url must be https")
    if m["file"].startswith("/") or ".." in m["file"].split("/"):
        sys.exit(f"manifest: {m['id']}: file must stay inside the models folder")
    print("\t".join(str(m[k]) for k in ["id", "file", "url", "sha256", "size_bytes", "license"]))
PY
}

wanted() {
  ((${#ids[@]} == 0)) && return 0
  local x
  for x in "${ids[@]}"; do [[ "$x" == "$1" ]] && return 0; done
  return 1
}

entries="$(read_manifest)"
mkdir -p "$models_dir"
failures=0
count=0

while IFS=$'\t' read -r id file url sha size license; do
  [[ -z "${id:-}" ]] && continue
  wanted "$id" || continue
  count=$((count + 1))
  dest="$models_dir/$file"
  if [[ -f "$dest" ]]; then
    got="$(sha256sum "$dest" | cut -d' ' -f1)"
    if [[ "$got" == "$sha" ]]; then
      echo "ok       $id ($license)"
    else
      echo "MISMATCH $id: $dest has sha256 $got, manifest says $sha" >&2
      failures=$((failures + 1))
    fi
    continue
  fi
  if ((verify_only)); then
    echo "MISSING  $id: $dest" >&2
    failures=$((failures + 1))
    continue
  fi
  free_bytes="$(df --output=avail -B1 "$models_dir" | tail -n 1 | tr -d ' ')"
  if ((free_bytes < size + 1073741824)); then
    echo "NOSPACE  $id needs $size bytes plus 1 GiB headroom; $free_bytes free" >&2
    failures=$((failures + 1))
    continue
  fi
  mkdir -p "$(dirname "$dest")"
  part="$dest.part"
  echo "fetch    $id <- $url"
  if ! curl --proto "$protocols" --proto-redir "$protocols" --tlsv1.2 -fL --retry 3 \
      --silent --show-error -o "$part" "$url"; then
    rm -f "$part"
    echo "FAILED   $id: download failed" >&2
    failures=$((failures + 1))
    continue
  fi
  got="$(sha256sum "$part" | cut -d' ' -f1)"
  if [[ "$got" != "$sha" ]]; then
    rm -f "$part"
    echo "MISMATCH $id: downloaded sha256 $got, manifest says $sha" >&2
    failures=$((failures + 1))
    continue
  fi
  chmod 0444 "$part"
  mv -f "$part" "$dest"
  echo "ok       $id ($license)"
done <<<"$entries"

if ((${#ids[@]} > 0 && count != ${#ids[@]})); then
  echo "fetch-models: some requested ids are not in the manifest" >&2
  failures=$((failures + 1))
fi
if ((failures)); then
  echo "fetch-models: FAIL - $failures problem(s)" >&2
  exit 1
fi
echo "fetch-models: ok - $count model(s) verified"
