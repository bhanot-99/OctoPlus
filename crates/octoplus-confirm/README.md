# octoplus-confirm

The confirmation dialog C6 starts for every `REVIEW` or `DANGEROUS` action (FR-KER-04, FR-KER-09; design.md §6; Architecture §8.4).

## Contract
C6 writes one JSON request to stdin and reads one decision line from stdout (`src/protocol.rs`). Exit 0 means a decision was written; anything else, or no valid line, is "not approved".

## Rules the window enforces
- Deny has the focus; Enter and Escape deny.
- Allow stays inactive for 1 s, re-armed each time the window regains focus.
- `DANGEROUS` needs a typed phrase.
- Wording comes only from `src/wording.rs`; colours and sizes from `ui/tokens/tokens.json` (via `build.rs`).

## Build
The library part (contract, wording, formatting) builds without GTK so C6 can share the types. The window is the `gui` feature, used by the binary; it uses the cairo renderer (ADR-0010).

## Self-tests
`octoplus-confirm --self-test <scenario>` runs scripted checks against the real window: `enter`, `allow-early`, `escape`, `expire`, `edit:<text>`, `danger`, `screenshot:<png path>`. Used by `tests/dialog_selftest.rs` (needs a Wayland session).
