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

# Linters: clippy, clang-format, clang-tidy, shellcheck, unused deps (NFR-QA-02).
lint: build
    cargo clippy --workspace --all-targets --locked -- -D warnings
    scripts/cpp-lint.sh
    shellcheck scripts/*.sh tests/scripts/*.sh
    cargo machete

# Unit, property and contract tests.
test: build
    cargo nextest run --workspace --locked --no-tests=fail
    cd cpp && ctest --preset {{cpp_preset}} --no-tests=error
    tests/scripts/fetch-models-test.sh

# Hosted CI subset: skips `local_*` tests that need a user session (rules.md §8.6).
test-hosted: build
    cargo nextest run --workspace --locked --no-tests=fail --profile hosted
    cd cpp && ctest --preset {{cpp_preset}} --no-tests=error
    tests/scripts/fetch-models-test.sh

# Dependency policy (advisories, bans, licences, sources) plus the network-ban probe.
deny:
    cargo deny check
    scripts/check-netban.sh

# Coverage gates, NFR-QA-01: Rust lines >= 80 %, C++ lines >= 70 %. Clean builds only.
cov: build
    cargo llvm-cov clean --workspace
    # Benchmark harnesses in bench/ are measuring tools, not product code: excluded from the gate.
    cargo llvm-cov nextest --workspace --exclude neuroos-bench-ipc-stream --exclude neuroos-bench-storage-scan --locked --fail-under-lines 80
    rm -rf cpp/build/coverage
    cd cpp && cmake --preset coverage && cmake --build --preset coverage --parallel
    cd cpp && ctest --preset coverage --no-tests=error
    gcovr --root cpp --filter 'cpp/libneuroos/src/' --object-directory cpp/build/coverage \
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
