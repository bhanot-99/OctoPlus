# octoplus-sandbox

Process self-restriction, the second layer behind each systemd unit (NFR-SEC-04; Architecture §8.2).

- `restrict`: Landlock on the declared paths, TCP denied; scope rules where the kernel has them. Fails closed if Landlock ABI v4 is not available (PRD §4: kernel ≥ 6.7).
- `lock_memory`: keeps key material out of swap.
- `child`: extra locks for skill children: `apply_limits` (rlimits) and `apply_seccomp` (allowlist; `unlink`, `rmdir`, `unlinkat` only for skills that may delete). Order: limits → `restrict` → seccomp, before reading input.
