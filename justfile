# Semantic-NeuroOS task runner — the single entry point (NFR-OPS-02, Architecture §14).
#   just ci         full local gate (reference machine), run before any story is Done
#   just ci-hosted  the subset hosted CI runs on every push

set shell := ["bash", "-euo", "pipefail", "-c"]
set positional-arguments := false

cpp_preset := "debug"

default:
    @just --list

# Stop early when disk is low (rules.md §9.8).
preflight:
    scripts/preflight.sh

# Format everything in place.
fmt:
    cargo fmt --all
    scripts/cpp-lint.sh --fix

# Check formatting without changing files.
fmt-check:
    cargo fmt --all --check
    scripts/cpp-lint.sh --check-format

# Configure the C++ build.
cpp-configure preset=cpp_preset:
    cd cpp && cmake --preset {{preset}}

# Build Rust and C++ (warnings are errors).
build: preflight (cpp-configure cpp_preset)
    cargo build --workspace --all-targets --locked
    cd cpp && cmake --build --preset {{cpp_preset}} --parallel
    scripts/build-voice.sh

# Linters: clippy, clang-format, clang-tidy, shellcheck, unused deps (NFR-QA-02).
lint: build
    cargo clippy --workspace --all-targets --locked -- -D warnings
    scripts/cpp-lint.sh
    shellcheck scripts/*.sh tests/scripts/*.sh
    cargo machete

# Unit, property and contract tests. Live tests stop and start the user's units; if
# neuroos.target was running before, it is started again afterwards, pass or fail.
test: build
    #!/usr/bin/env bash
    set -euo pipefail
    was_active=0
    systemctl --user is-active --quiet neuroos.target && was_active=1
    restore() { if [[ $was_active == 1 ]]; then systemctl --user start neuroos.target || true; fi; }
    trap restore EXIT
    cargo nextest run --workspace --locked --no-tests=fail
    (cd cpp && ctest --preset {{cpp_preset}} --no-tests=error)
    tests/scripts/fetch-models-test.sh
    # C7's real HTTPS transport against a local server in a private network namespace (rules.md §8.4).
    tests/scripts/fetcher-netns-test.sh
    tests/scripts/fetcher-firewall-test.sh

# Hosted CI subset: skips `local_*` tests that need a user session (rules.md §8.6).
test-hosted: build
    cargo nextest run --workspace --locked --no-tests=fail --profile hosted
    cd cpp && ctest --preset {{cpp_preset}} --no-tests=error -LE local
    tests/scripts/fetch-models-test.sh

# Dependency policy (advisories, bans, licences, sources) plus the network-ban probe.
deny:
    cargo deny check
    scripts/check-netban.sh
    scripts/check-octopit.sh

# Coverage gates, NFR-QA-01: Rust lines >= 80 %, C++ lines >= 70 %. Clean builds only.
cov: build
    #!/usr/bin/env bash
    set -euo pipefail
    # The Rust tests stop and start the real units; leave NeuroOS the way it was found (as `test` does).
    was_active=0
    systemctl --user is-active --quiet neuroos.target && was_active=1
    restore() { if [[ $was_active == 1 ]]; then systemctl --user start neuroos.target || true; fi; }
    trap restore EXIT
    cargo llvm-cov clean --workspace
    # Benchmark harnesses in bench/ are measuring tools, not product code: excluded from the gate.
    cargo llvm-cov nextest --workspace --exclude neuroos-bench-ipc-stream --exclude neuroos-bench-storage-scan --exclude neuroos-bench-kernel-check --locked --fail-under-lines 80
    # Child processes started with a cleared environment write default-named profiles into the crate folders.
    find crates -name '*.profraw' -delete
    rm -rf cpp/build/coverage
    (cd cpp && cmake --preset coverage && cmake --build --preset coverage --parallel)
    (cd cpp && ctest --preset coverage --no-tests=error)
    gcovr --root cpp --filter 'cpp/libneuroos/src/' --filter 'cpp/neuroos-inference/src/' --filter 'cpp/neuroos-voice/src/' --object-directory cpp/build/coverage \
        --gcov-ignore-parse-errors=negative_hits.warn --fail-under-line 70 --print-summary

# C++ tests under AddressSanitizer+UBSan and ThreadSanitizer (Architecture §14).
sanitize: preflight
    cd cpp && cmake --preset asan && cmake --build --preset asan --parallel
    cd cpp && ctest --preset asan --no-tests=error
    cd cpp && cmake --preset tsan && cmake --build --preset tsan --parallel
    cd cpp && ctest --preset tsan --no-tests=error

# Benchmarks; results go to reports/bench/ (rules.md §9.6).
bench: build
    cargo bench --workspace --locked

# Spike S-01: SQLite FTS5 and exact in-memory vector scan benchmark (P0-S07).
bench-s01:
    scripts/bench-s01.sh

# Spike S-02: build upstream llama.cpp (CPU and Vulkan) out-of-tree (P0-S08).
build-llama *args:
    scripts/build-llama.sh {{args}}

# Spike S-02: run candidate model bake-off on CPU (and Vulkan if built) (P0-S08).
bakeoff-s02 *args:
    scripts/bakeoff-s02.sh {{args}}

# Spike S-03: measure streamed text frames and cancel latency (P0-S09).
bench-s03:
    scripts/bench-s03.sh

# Spike S-04: systemd hardening, socket isolation, peer checks, and GPU access (P0-S10).
spike-s04:
    scripts/spike-s04.sh

# Spike S-05: sensor sources, fullscreen state, and microphone-in-use detection (P0-S11).
spike-s05:
    scripts/spike-s05.sh

# Fetch and verify every model listed in models/manifest.toml (P0-S06).
fetch-models *args:
    scripts/fetch-models.sh {{args}}

# Generate systemd unit files from the socket map (P1-S01, P1-S02).
units:
    cargo run -p neuroos-deploy --bin neuroos-gen-units

# Install developer units and binaries into ~/.local (P1-S02).
install-dev:
    scripts/install-dev.sh

# Build Debian package (.deb) (Phase 10, P10-S01).
deb *args:
    scripts/package-deb.sh {{args}}

# Production installer and uninstaller (Phase 10, P10-S02).
install *args:
    scripts/install.sh {{args}}

uninstall *args:
    scripts/uninstall.sh {{args}}

# Check reproducible build and threat-model verification (Phase 10, P10-S06).
check-reproducible:
    scripts/check-reproducible.sh

# Run the three security proofs and check egress isolation (P1-S01..S04).
sc:
    #!/usr/bin/env bash
    set -euo pipefail
    # The proofs stop the real units. The egress check needs them running, so start them again
    # first, and leave NeuroOS the way it was found at the end (as `test` does).
    was_active=0
    systemctl --user is-active --quiet neuroos.target && was_active=1
    restore() { if [[ $was_active == 1 ]]; then systemctl --user start neuroos.target || true; fi; }
    trap restore EXIT
    cargo nextest run -p neuroos-testkit --test local_sc_proof_1_net --test local_sc_proof_2_socket_map --test local_sc_proof_3_landlock
    systemctl --user start neuroos.target
    for _ in $(seq 1 90); do
      all=1
      for u in healthd storage inference knowledge kernel monitor voice; do
        systemctl --user is-active --quiet "neuroos-$u.service" || all=0
      done
      [[ $all == 1 ]] && break
      sleep 1
    done
    scripts/check-egress.sh

# Free disk: remove build output.
prune:
    cargo clean
    rm -rf cpp/build target/llvm-cov-target
    @df -h . | tail -n 1

# Hosted CI: format, lint, build, unit tests and dependency checks.
ci-hosted: fmt-check lint test-hosted deny

# Full local gate. Every step must run to the end (rules.md §2.9).
ci: fmt-check lint test deny cov sanitize
    @echo "just ci: ALL GREEN"

# Phase 1 benchmark suite: conditions, C4 one-shot bench, RSS, e2e latency, cold-start (P1-PF).
bench-p1 *args:
    scripts/bench-p1.sh {{args}}

# Phase 1 2-hour mini-soak driver (P1-MS).
soak-p1 *args:
    scripts/soak-p1.sh {{args}}

