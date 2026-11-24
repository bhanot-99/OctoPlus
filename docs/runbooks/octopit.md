# Runbook — OctoPit, the desktop dashboard

**Use when:** you want to open, update or troubleshoot the dashboard. OctoPit only reads status and sends your typed questions. It cannot approve anything and it never touches audio.

## Open it
- From the launcher: search for "OctoPit". Or in a terminal: `octopit`.
- Shortcuts: `Ctrl+1` to `Ctrl+5` pages, `Ctrl+,` settings, `Ctrl+Space` mic button, `Ctrl+K` command palette, `Esc` stop an answer.

## It says "OctoPlus is not running"
1. `octoplusctl status` — if that also fails, follow `docs/runbooks/a-part-wont-start.md`.
2. `systemctl --user start octoplus.target`, wait a few seconds; the window recovers by itself.

## The mic button is grey
- "Voice is off": `systemctl --user start octoplus-voice`.
- "Another app is using the microphone": close that app; the button comes back by itself.
- "Voice is unavailable": the voice part is not answering; see `octoplusctl voice status` and the journal: `journalctl --user -u octoplus-voice -e`.

## The GPU card says "No supported GPU found"
OctoPit reads `/usr/bin/nvidia-smi` for NVIDIA cards. Without it, or without a driver, the card says so; OctoPlus runs on the CPU either way.

## Settings
Stored in `~/.config/octoplus/octopit.json` (mode 0600). Delete the file to reset. `octoplusctl forget` does not touch it because it holds no personal data.

## Chat history
There is none on disk: closing the window ends the conversation. Answers are shown as plain text only.

## Update or remove
Installed and removed with the rest of OctoPlus (`scripts/install.sh`, the `.deb`, `scripts/uninstall.sh`). Nothing else to clean up.
