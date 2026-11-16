# neuroos-skill

The sandboxed child that runs one skill for C6 (FR-KER-05; Architecture §8.2; ADR-0010 item 2).

## Order (before any untrusted input is read)
1. Parse the trusted command line from C6 (skill, folders, limits).
2. Resource limits.
3. Landlock on exactly the declared folders, TCP denied.
4. seccomp allowlist (`neuroos-sandbox::child`): no `socket`, `connect`, `execve`, `fork`/`clone`, `ptrace` or `kill`; anything else not listed fails with `EPERM`.
5. Read the arguments (already schema-checked and approved) from stdin, run, print one JSON line: `{"ok":true,"result":…}` or `{"ok":false,"error":"<code>"}`.

Exit codes: 0 = a line was written (it may report a skill error); 2 = bad command line or input; 3 = the sandbox could not be applied (nothing was read or done).

## Skills
- `system.read_metadata`, `system.disk_report`: read-only.
- `system.cleanup`: `plan` mode lists every entry and size (read-only); `run` mode re-checks every approved path and can only delete entries strictly inside the trash, `~/.cache` and configured build folders (symlinks are deleted as links, never followed). Deletion is permanent (ADR-0010 item 8).

## Tests
`tests/escape.rs` proves each forbidden thing (reading the key, other folders, network, starting programs) fails inside the sandbox and succeeds without it, using the `probe` binary.
