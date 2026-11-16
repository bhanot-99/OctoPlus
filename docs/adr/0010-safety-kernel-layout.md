# ADR-0010 — Safety kernel layout: compiled-in skills, dialog and skill children, C6 sandbox as the union

**Status:** Proposed — decided by Claude under the owner's Phase 2 go-ahead (D-23); the owner accepts or overturns it at Phase 2 sign-off
**Date:** 2026-11-16
**Changes:** Architecture §7.1 (audit folder), §8.2 (C6 and skill-runner rows), §8.4 (who verifies), §16 (`neuroos-confirm`, `ui/tokens`) · PRD §5.9 (`system.cleanup` row), §6.3 (NFR-SEC-09, NFR-SEC-10), §12 OQ-07 · design.md §6.1 (cleanup wording), §6 (wording table lives in code), §11 (tokens file created) · security.md §4.4, §6
**Evidence:** GTK4 window RSS on the reference machine, 2026-11-16, release build, one 480 px dialog, measured from `/proc/self/status` after 4 s: default renderer 162 MiB RSS (44 MiB anonymous); `GSK_RENDERER=cairo` 66 MiB RSS (9 MiB anonymous, 34 MiB file-backed). Not yet measured inside C6's unit (Phase 2 gate).

---

## Context

Phase 2 builds C6. Four questions were left open by Architecture §8:

1. Where do skill manifests live? Architecture §8.2 lists "skill manifests" as a C6 read-only path, so a file edit could change a tier.
2. How do skills that only send one message to another part (`privacy.pause` → C1 and C2 control) run "in a sandboxed child"? A child would need the peer sockets, which widens the child, not narrows it.
3. Landlock and the unit's mount namespace are inherited by every child and can only be narrowed. The dialog needs the Wayland socket and the system's fonts and themes. `system.disk_report` and `system.cleanup` need the cleanup folders. C6's own row in §8.2 lists none of these, so as written the children could not work.
4. How heavy is a GTK4 dialog against C6's 64 MiB hard limit?

## Decision

1. **Skill manifests are compiled into the kernel** (`crates/neuroos-kernel/skills/*.toml`, `include_str!`). They are parsed and validated at start-up; any error is fatal. All 13 v1.0 skills are registered so every tier can be answered. Skills not built yet answer `evaluate` but are refused at dispatch. C6 needs no manifest path at run time.
2. **Two kinds of skill.** `child` skills (`system.read_metadata`, `system.disk_report`, `system.cleanup`) run in a separate `neuroos-skill` process with a narrower Landlock policy, a seccomp filter, resource limits (address space, CPU time, open files, processes, file size) and a wall-clock timeout enforced by C6. `kernel` skills (`privacy.pause`) are one schema-validated message that C6 sends itself on the control sockets it already holds (Architecture §5.2). They have no file access and are audited exactly like child skills.
3. **C6's sandbox is the union of what its children need.** Each child then narrows itself before reading input:
   - C6 unit: adds bind mounts for the Wayland socket (dialog), the session-bus socket (notifications), and the cleanup roots. Landlock read-only adds `/usr`, `/etc/fonts` and the dialog and skill binaries. Read-write adds the cleanup roots and the trash.
   - Dialog child: runs with `GSK_RENDERER=cairo` (no GPU node, so `PrivateDevices=true` stays) and inherits C6's policy. It never sees `hmac.key`: C6 opens the key before it starts any child, and the key file is not in any child's Landlock rules.
   - Skill child: Landlock only on its own declared roots (`metadata`, `cleanup_roots`, `trash`). `hmac.key`, the logs and every socket are outside its policy.
   - The alternative of a pre-sandbox "spawner" process that keeps the wide rights while C6 narrows itself is recorded below and revisited in Phase 9.
4. **The dialog is `neuroos-confirm`**, a separate binary in the C6 boundary. It gets one JSON request on stdin and writes one JSON decision line to stdout (`neuroos_confirm::protocol`). All wording is fixed in the dialog's own code, keyed by capability. The request carries only data: paths, sizes, validated arguments, hosts, the drafted query. Colours come from `ui/tokens/tokens.json` through a build step. The approve button is re-disabled for the delay **every time the window regains focus**, not only on open. Icons are plain Unicode glyphs for now; vendoring Lucide SVGs is deferred.
5. **Approval tokens are created and verified in C6.** The kernel's executor re-verifies the token with a constant-time compare immediately before it starts a child. The child never holds a key. C7 verifies its own tokens with `fetch.key` in Phase 8.
6. **`hmac.key` is created by the installer** (`scripts/install-dev.sh`): 32 bytes from the OS random source, mode 0600. C6 refuses to start if the key is missing, the wrong size, or readable by group or others. `neuroosctl rotate-keys` and `fetch.key` arrive in Phase 8, when there is a second key to rotate together.
7. **OQ-07 closed:** the cleanup allowlist is the trash plus `~/.cache` by default. Build folders are added only through `[kernel] cleanup_build_folders` in `config.toml`, which is empty by default. The owner named none when Phase 2 started.
8. **Cleanup deletes; it does not move to the trash.** Inside C6's unit, `~/.cache`, the trash and each build folder are separate bind mounts. `rename` between mounts fails (`EXDEV`), so "move to trash" would mean copying every byte first: slow, and it needs free space on a disk that is already full. Cleanup therefore permanently deletes the listed cache and build entries (apps recreate them) and empties the trash. The dialog says "permanently delete", lists every item with its size, and the button reads "Delete". PRD FR-KER-10 allowed "moves to trash where possible"; in this sandbox it is not possible.
9. **Audit files live in `~/.local/share/neuroos/audit/`** (`actions.jsonl`, `egress.jsonl` and their rotations), so C6's unit binds exactly that folder and never the data root, where C3's database lives.
10. **New kernel limits** (PRD NFR-SEC-09 and NFR-SEC-10, labelled T): REVIEW and DANGEROUS requests at most 6 per rolling minute; a request the user denied is refused for 300 s; approve-button delay 1 s; one dialog at a time.
11. **`llm.generate` is not a skill.** C5 asks C6 about it before every generation (Architecture §6.1). Generating text runs nothing, so C6 answers SAFE for any valid taint and never escalates it; a missing or unknown taint is still refused. It is not in the registry, so it can never be dispatched. Taint stays on the generated text and decides the tier when a skill is dispatched.

## Consequences

- A compromised C6 process has read-write access to the cleanup roots and can reach the Wayland socket. A compromised C5 still cannot: it can only ask, and every non-SAFE action needs the dialog.
- The dialog's RSS (about 66 MiB with the cairo renderer, outside the unit) exceeds the PRD's "+25 MiB while a dialog is open". cgroup accounting differs from RSS (shared library pages already charged elsewhere are not counted again), so the real number inside C6's unit is measured at the Phase 2 gate. If it breaks the 64 MiB hard limit, the owner decides: raise the budget, or move the dialog to its own unit.
- Tiers change only by rebuilding the binary.
- `privacy.pause` does not run in a child process. This is a documented narrowing of FR-KER-05 for skills with no file or network access.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| Manifests read from disk at run time | A file edit could lower a tier; a compiled table has no such path. |
| A spawner process forked before C6 restricts itself, so C6 keeps a narrow policy | Better isolation of the parsing process, but a second privileged process and protocol inside C6. Revisit in Phase 9 with the hardening score. |
| Start the dialog and skills as transient systemd units (`systemd-run --user`) | Needs access to the user manager's private bus, which can start any unit: a far larger escape than the union policy. |
| GTK default renderer | 162 MiB RSS and needs the GPU device nodes, which `PrivateDevices=true` hides. |
| Run `privacy.pause` in a child with the control sockets bound in | Widens the child to reach other parts. The message is fully validated, so a child adds nothing. |
