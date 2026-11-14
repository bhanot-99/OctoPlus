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
    shellcheck scripts/*.sh
    cargo machete

# Unit, property and contract tests.
test: build
    cargo nextest run --workspace --locked --no-tests=fail
    cd cpp && ctest --preset {{cpp_preset}} --no-tests=error

# Dependency policy (advisories, bans, licences, sources) plus the network-ban probe.
deny:
    cargo deny check
    scripts/check-netban.sh

# Coverage gates, NFR-QA-01: Rust lines >= 80 %, C++ lines >= 70 %. Clean builds only.
cov: build
    cargo llvm-cov clean --workspace
    cargo llvm-cov nextest --workspace --locked --fail-under-lines 80
    rm -rf cpp/build/coverage
    cd cpp && cmake --preset coverage && cmake --build --preset coverage --parallel
    cd cpp && ctest --preset coverage --no-tests=error
    gcovr --root cpp --filter 'cpp/libneuroos/src/' --object-directory cpp/build/coverage \
        --gcov-ignore-parse-errors=negative_hits.warn --fail-under-line 70 --print-summary

# Benchmarks; results go to reports/bench/ (rules.md §9.6).
bench: build
    cargo bench --workspace --locked

# Fetch and verify every model listed in models/manifest.toml (P0-S06).
fetch-models *args:
    scripts/fetch-models.sh {{args}}

# Free disk: remove build output.
prune:
    cargo clean
    rm -rf cpp/build target/llvm-cov-target
    @df -h . | tail -n 1

# Hosted CI: format, lint, build, unit tests and dependency checks.
ci-hosted: fmt-check lint test deny

# Full local gate. Every step must run to the end (rules.md §2.9).
ci: fmt-check lint test deny cov
    @echo "just ci: ALL GREEN"
