# Runbook — Restore a Backup

This runbook guides operators through disaster recovery and restoring Semantic-NeuroOS databases, ledgers, and configurations from backup snapshots (Architecture §13, rules.md §11).

---

## 1. Backup Artifacts Layout

Semantic-NeuroOS automated backups are triggered by `neuroos-storage-backup.timer` and stored with permissions `0700` in:
```
~/.local/share/neuroos/backups/
├── storage-20261121T180000Z.db       # Hot SQLite snapshot (VACUUM INTO)
├── actions-20261121T180000Z.jsonl    # Hash-chained action audit ledger
└── egress-20261121T180000Z.jsonl     # Hash-chained network egress ledger
```

Cryptographic keys and user configuration are located in:
```
~/.config/neuroos/
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
systemctl --user stop neuroos.target
```
Confirm no dangling instances remain:
```bash
pgrep -l neuroos || true
```

### Step 2: Select Backup Snapshot
Identify the target snapshot timestamp to restore:
```bash
ls -lt ~/.local/share/neuroos/backups/
```
Set the chosen timestamp:
```bash
SNAPSHOT="20261121T180000Z"
```

### Step 3: Restore Database & Ledgers
1. Archive current corrupted/broken state:
   ```bash
   mkdir -p ~/.local/share/neuroos/storage/corrupt-$(date +%s)
   mv ~/.local/share/neuroos/storage/storage.db* ~/.local/share/neuroos/storage/corrupt-*/ 2>/dev/null || true
   ```
2. Copy backup into place:
   ```bash
   cp ~/.local/share/neuroos/backups/storage-${SNAPSHOT}.db ~/.local/share/neuroos/storage/storage.db
   chmod 0600 ~/.local/share/neuroos/storage/storage.db
   ```
3. If restoring action or egress ledgers:
   ```bash
   if [[ -f ~/.local/share/neuroos/backups/actions-${SNAPSHOT}.jsonl ]]; then
     cp ~/.local/share/neuroos/backups/actions-${SNAPSHOT}.jsonl ~/.local/share/neuroos/kernel/actions.jsonl
     chmod 0600 ~/.local/share/neuroos/kernel/actions.jsonl
   fi
   if [[ -f ~/.local/share/neuroos/backups/egress-${SNAPSHOT}.jsonl ]]; then
     cp ~/.local/share/neuroos/backups/egress-${SNAPSHOT}.jsonl ~/.local/share/neuroos/kernel/egress.jsonl
     chmod 0600 ~/.local/share/neuroos/kernel/egress.jsonl
   fi
   ```

### Step 4: Verify Integrity
1. Run SQLite integrity check:
   ```bash
   sqlite3 ~/.local/share/neuroos/storage/storage.db "PRAGMA integrity_check;"
   ```
   Output must be `ok`.
2. Verify foreign keys and quick check:
   ```bash
   sqlite3 ~/.local/share/neuroos/storage/storage.db "PRAGMA foreign_key_check; PRAGMA quick_check;"
   ```
   Both must return empty / `ok`.

---

## 3. Restoring on a New Workstation

If restoring to a replacement machine:
1. Complete fresh install per [install.md](install.md).
2. Restore `~/.config/neuroos/hmac.key` and `~/.config/neuroos/fetch.key` with mode `0600`.
3. If restoring system `fetch.key`:
   ```bash
   sudo cp ~/.config/neuroos/fetch.key /etc/neuroos/fetch.key
   sudo chown root:neuroos /etc/neuroos/fetch.key
   sudo chmod 0640 /etc/neuroos/fetch.key
   ```
4. Restore `~/.local/share/neuroos/storage/storage.db`.
5. Start services: `systemctl --user start neuroos.target`.
6. Verify status: `neuroosctl status`.
