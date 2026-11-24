# Runbook — record an anonymised monitor session

For the Phase 3 measurement test (MS) and the Phase 4 noise gate. The recording is **anonymised before it is written** (FR-MON-12): window titles, media tags, file paths and process names become keyed placeholders, and the key is thrown away when the recording stops. Nothing readable about what you did is stored, but the *shape* of your day is (which public apps, when, for how long). Look at `scripts/check-dump.sh` output before you share or commit any dump.

## What is needed

- Phase 3 installed: `cargo build --release -p octoplus-monitor -p octoplusctl` and the install steps in `scripts/install-dev.sh` (binaries in `~/.local/opt/octoplus/bin`, units in `~/.config/systemd/user`).
- Your normal desktop session. Work as you normally do. The monitor adds a few MiB of memory and almost no CPU.
- For the performance run: set the CPU governor to `performance` first (`sudo cpupower frequency-set -g performance`) and write the governor into the report. The recording itself does not need it.

## What the test needs (phases.md §6.3 MS)

At least **8 hours in total across at least 3 dumps**, plus **one separate dump held out** for evaluation. The held-out dump is never used to tune anything (rules.md §8.8). A dump is a session: start in the morning, stop at lunch, start again, and so on. Pausing inside a session (`octoplusctl` / the privacy pause) is fine: paused time is simply absent.

## Steps

```bash
scripts/record-session.sh start morning          # session 1 (repeat with other labels)
scripts/record-session.sh status
scripts/record-session.sh stop                    # validates the dump when it ends

scripts/record-session.sh start --held-out eval   # ONE held-out session, kept aside
scripts/record-session.sh stop

scripts/check-dump.sh ~/.local/share/octoplus/recordings/morning-*.dump
```

`stop` cleanly stops C1 (that writes the dump's end record), checks the file with `octoplusctl replay`, and starts C1 normally again. If the machine loses power mid-session the dump has no end record and `replay` refuses it; the events before the crash are not usable, so record shorter sessions rather than one very long one.

While recording, `~/.local/share/octoplus/recordings/<label>.metrics.jsonl` gets one line a minute with C1's RSS, CPU and p99 capture latency, taken from `octoplusctl status`. Those lines are the evidence for the "RSS over the run" figure in the phase report.

## Before anything is committed

1. `scripts/check-dump.sh <dump>` must exit 0, and you approve the list of readable app ids it prints.
2. Copy only approved, **non-held-out** dumps to `tests/fixtures/monitor-dumps/`. Keep the held-out dump where it is and write its path into the phase report; do not commit it, do not replay it while tuning.
3. Raw recordings do not exist: there is no raw mode.

## If something goes wrong

- `stop` says the dump is incomplete: the monitor was killed (not stopped). Discard it and record again.
- `status` says recording but the unit is down: `systemctl --user status octoplus-monitor`, then remove `$XDG_RUNTIME_DIR/systemd/user/octoplus-monitor.service.d/record.conf` and `~/.local/share/octoplus/recordings/.current`, and run `systemctl --user daemon-reload`.
