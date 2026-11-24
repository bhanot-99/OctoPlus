# Runbook — Restore a Backup

This runbook guides operators through disaster recovery and restoring OctoPlus databases, ledgers, and configurations from backup snapshots (Architecture §13, rules.md §11).

---

## 1. Backup Artifacts Layout

OctoPlus automated backups are triggered by `octoplus-storage-backup.timer` and stored with permissions `0700` in:
```
~/.local/share/octoplus/backups/
├── storage-20261121T180000Z.db       # Hot SQLite snapshot (VACUUM INTO)
├── actions-20261121T180000Z.jsonl    # Hash-chained action audit ledger
└── egress-20261121T180000Z.jsonl     # Hash-chained network egress ledger
```

Cryptographic keys and user configuration are located in:
```
~/.config/octoplus/
├── config.toml
├── nudges.toml
├── hmac.key
└── fetch.key
```

---

## 2. Restore Steps

### Step 1: Stop Services
Before modifying database or ledger files on disk, ensure that all services writing to storage or kernel ledgers are stopped:
```bash
systemctl --user stop octoplus.target
```
Confirm no dangling instances remain:
```bash
pgrep -l octoplus || true
```

### Step 2: Select Backup Snapshot
Identify the target snapshot timestamp to restore:
```bash
ls -lt ~/.local/share/octoplus/backups/
```
Set the chosen timestamp:
```bash
SNAPSHOT="20261121T180000Z"
```

### Step 3: Restore Database & Ledgers
1. Archive current corrupted/broken state:
   ```bash
   mkdir -p ~/.local/share/octoplus/storage/corrupt-$(date +%s)
   mv ~/.local/share/octoplus/storage/storage.db* ~/.local/share/octoplus/storage/corrupt-*/ 2>/dev/null || true
   ```
2. Copy backup into place:
   ```bash
   cp ~/.local/share/octoplus/backups/storage-${SNAPSHOT}.db ~/.local/share/octoplus/storage/storage.db
   chmod 0600 ~/.local/share/octoplus/storage/storage.db
   ```
3. If restoring action or egress ledgers:
   ```bash
   if [[ -f ~/.local/share/octoplus/backups/actions-${SNAPSHOT}.jsonl ]]; then
     cp ~/.local/share/octoplus/backups/actions-${SNAPSHOT}.jsonl ~/.local/share/octoplus/kernel/actions.jsonl
     chmod 0600 ~/.local/share/octoplus/kernel/actions.jsonl
   fi
   if [[ -f ~/.local/share/octoplus/backups/egress-${SNAPSHOT}.jsonl ]]; then
     cp ~/.local/share/octoplus/backups/egress-${SNAPSHOT}.jsonl ~/.local/share/octoplus/kernel/egress.jsonl
     chmod 0600 ~/.local/share/octoplus/kernel/egress.jsonl
   fi
   ```

### Step 4: Verify Integrity
1. Run SQLite integrity check:
   ```bash
   sqlite3 ~/.local/share/octoplus/storage/storage.db "PRAGMA integrity_check;"
   ```
   Output must be `ok`.
2. Verify foreign keys and quick check:
   ```bash
   sqlite3 ~/.local/share/octoplus/storage/storage.db "PRAGMA foreign_key_check; PRAGMA quick_check;"
   ```
   Both must return empty / `ok`.

---

## 3. Restoring on a New Workstation

If restoring to a replacement machine:
1. Complete fresh install per [install.md](install.md).
2. Restore `~/.config/octoplus/hmac.key` and `~/.config/octoplus/fetch.key` with mode `0600`.
3. If restoring system `fetch.key`:
   ```bash
   sudo cp ~/.config/octoplus/fetch.key /etc/octoplus/fetch.key
   sudo chown root:octoplus /etc/octoplus/fetch.key
   sudo chmod 0640 /etc/octoplus/fetch.key
   ```
4. Restore `~/.local/share/octoplus/storage/storage.db`.
5. Start services: `systemctl --user start octoplus.target`.
6. Verify status: `octoplusctl status`.
