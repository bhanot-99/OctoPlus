# Runbook — Upgrade Semantic-NeuroOS

This runbook guides operators through upgrading Semantic-NeuroOS binaries, schema migrations, and model updates without data loss.

---

## 1. Pre-Upgrade Safeguards

Before upgrading any binaries or configuration:

1. **Create an Immediate Database Snapshot:**
   ```bash
   neuroosctl storage backup
   # Verify backup created in ~/.local/share/neuroos/backups/
   ls -lh ~/.local/share/neuroos/backups/
   ```
2. **Record Current Component State:**
   ```bash
   neuroosctl status --json > /tmp/neuroos-pre-upgrade-status.json
   ```
3. **Stop Active Services:**
   ```bash
   systemctl --user stop neuroos.target
   sudo systemctl stop neuroos-fetcher.service
   ```

---

## 2. Binary Upgrade

### If using Debian Package (.deb):
```bash
sudo dpkg -i dist/semantic-neuroos_<new_version>_amd64.deb
```

### If using Direct Script:
```bash
sudo scripts/install.sh --system
scripts/install.sh --user
```

---

## 3. Storage Migrations & Integrity Check

Semantic-NeuroOS applies SQLite migrations sequentially using `PRAGMA user_version`. When `neuroos-storage` boots, it checks and applies pending migrations automatically.

1. Test database integrity:
   ```bash
   sqlite3 ~/.local/share/neuroos/storage/storage.db "PRAGMA integrity_check;"
   ```
   Must output `ok`.
2. Inspect current migration version:
   ```bash
   sqlite3 ~/.local/share/neuroos/storage/storage.db "PRAGMA user_version;"
   ```

---

## 4. Model Weights Check

If the new release updates `models/manifest.toml`:
```bash
just fetch-models
# Verify checksums match ~/.local/opt/neuroos/models/SHA256SUMS
sha256sum --check --quiet ~/.local/opt/neuroos/models/SHA256SUMS
```

---

## 5. Restart & Post-Upgrade Validation

1. Reload systemd daemon and restart target:
   ```bash
   systemctl --user daemon-reload
   sudo systemctl daemon-reload
   sudo systemctl start neuroos-fetcher.service
   systemctl --user start neuroos.target
   ```
2. Validate running services:
   ```bash
   neuroosctl status
   ```
3. Verify action audit and egress integrity:
   ```bash
   neuroosctl audit verify
   ```

---

## 6. Rollback Procedure

If the upgraded version fails to start or encounters regressions:
1. Stop services: `systemctl --user stop neuroos.target`
2. Reinstall previous `.deb` or restore binaries from `target/release`.
3. If database schema was modified, restore backup:
   ```bash
   cp ~/.local/share/neuroos/backups/storage-<timestamp>.db ~/.local/share/neuroos/storage/storage.db
   ```
4. Restart services: `systemctl --user start neuroos.target`.
