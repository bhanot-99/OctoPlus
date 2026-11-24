# Runbook — Forget Everything (Privacy Purge)

This runbook guides operators through an immediate, irreversible data wipe ("forget everything"), erasing all conversational memory, search history, vector embeddings, sensor telemetry, and access ledgers (PRD FR-PRV-05, rules.md §11).

---

## 1. Scope of the Wipe

Executing "forget everything" permanently destroys:
1. **Long-Term Memory & Vector Index:** `~/.local/share/octoplus/storage/storage.db` (and all WAL/SHM files).
2. **Conversation Transcripts:** Any stored chat sessions, prompt caches, and context windows.
3. **Sensor Telemetry Dumps:** `~/.local/share/octoplus/recordings/` and `~/.local/share/octoplus/soak/`.
4. **Action & Egress Ledgers:** `actions.jsonl` and `egress.jsonl`.
5. **Network Fetcher Spool:** `/var/spool/octoplus-fetcher/`.
6. **Backups:** `~/.local/share/octoplus/backups/`.

It **preserves** installed binaries (`/opt/octoplus/bin`) and base configuration templates so the buddy can restart fresh without reinstalling.

---

## 2. One-Command Complete Purge

If you wish to uninstall and purge everything:
```bash
scripts/uninstall.sh --purge --yes
```

---

## 3. In-Place Privacy Purge (Reset Without Uninstall)

If you wish to keep OctoPlus running but wipe all memory and history back to day 1:

### Step 1: Stop Services
```bash
systemctl --user stop octoplus.target
```

### Step 2: Delete All Databases, Embeddings & Logs
```bash
# Wipe storage and databases
rm -rf ~/.local/share/octoplus/storage/*
rm -rf ~/.local/share/octoplus/backups/*
rm -rf ~/.local/share/octoplus/recordings/*
rm -rf ~/.local/share/octoplus/soak/*
rm -rf ~/.local/share/octoplus/kernel/*

# Wipe ephemeral spool (requires sudo if clearing system fetcher spool)
if [[ -d /var/spool/octoplus-fetcher ]]; then
  sudo rm -rf /var/spool/octoplus-fetcher/*
fi

# Clear user cache & runtime sockets
rm -rf ~/.cache/octoplus/*
rm -rf "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/octoplus"
```

### Step 3: Re-create Clean Storage Directories
```bash
mkdir -p ~/.local/share/octoplus/storage
mkdir -p ~/.local/share/octoplus/backups
chmod 0700 ~/.local/share/octoplus ~/.local/share/octoplus/storage ~/.local/share/octoplus/backups
```

### Step 4: Restart with Blank Memory
```bash
systemctl --user start octoplus.target
```

### Step 5: Verification
1. Check that storage starts with a brand new, empty database:
   ```bash
   sqlite3 ~/.local/share/octoplus/storage/storage.db "SELECT count(*) FROM chunks;"
   # Expected: 0
   ```
2. Verify profile facts are empty:
   ```bash
   octoplusctl profile list
   # Expected: empty
   ```
3. Check egress ledger is clean:
   ```bash
   octoplusctl egress list
   # Expected: empty
   ```
4. Verify overall system health:
   ```bash
   octoplusctl status
   # Expected: all OK
   ```
