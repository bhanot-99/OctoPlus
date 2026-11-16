-- C3 schema, version 1 (Architecture §7.2). Every taint column is NOT NULL and range-checked:
-- a missing taint is an error, never "clean" (R0-4).

CREATE TABLE index_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE focus_history (
    id       INTEGER PRIMARY KEY,
    app_id   TEXT    NOT NULL,
    title    TEXT    NOT NULL,
    pid      INTEGER NOT NULL DEFAULT 0,
    start_ns INTEGER NOT NULL,
    end_ns   INTEGER NOT NULL CHECK (end_ns >= start_ns),
    dwell_ns INTEGER NOT NULL CHECK (dwell_ns >= 0),
    taint    INTEGER NOT NULL CHECK (taint BETWEEN 0 AND 15)
);
CREATE INDEX focus_history_time ON focus_history (start_ns, end_ns);
CREATE INDEX focus_history_app  ON focus_history (app_id);

CREATE TABLE event_counters (
    key         TEXT PRIMARY KEY,
    domain      INTEGER NOT NULL,
    app_id      TEXT    NOT NULL DEFAULT '',
    occurrences INTEGER NOT NULL,
    dwell_ns    INTEGER NOT NULL,
    first_ns    INTEGER NOT NULL,
    last_ns     INTEGER NOT NULL,
    promoted    INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;
CREATE INDEX event_counters_last ON event_counters (last_ns);

CREATE TABLE entities (
    id            INTEGER PRIMARY KEY,
    domain        INTEGER NOT NULL,
    kind          TEXT    NOT NULL,
    label         TEXT    NOT NULL,
    app_id        TEXT    NOT NULL DEFAULT '',
    taint         INTEGER NOT NULL CHECK (taint BETWEEN 0 AND 15),
    first_seen_ns INTEGER NOT NULL,
    last_seen_ns  INTEGER NOT NULL,
    UNIQUE (domain, kind, label)
);
CREATE INDEX entities_app  ON entities (app_id);
CREATE INDEX entities_last ON entities (last_seen_ns);

CREATE TABLE chunks (
    id          INTEGER PRIMARY KEY,
    entity_id   INTEGER NOT NULL REFERENCES entities (id) ON DELETE CASCADE,
    domain      INTEGER NOT NULL,
    text        TEXT    NOT NULL,
    text_hash   INTEGER NOT NULL,
    taint       INTEGER NOT NULL CHECK (taint BETWEEN 0 AND 15),
    at_ns       INTEGER NOT NULL,
    source      TEXT    NOT NULL,
    app_id      TEXT    NOT NULL DEFAULT '',
    vector      BLOB,
    embed_model TEXT    NOT NULL DEFAULT '',
    UNIQUE (entity_id, text_hash)
);
CREATE INDEX chunks_time   ON chunks (at_ns);
CREATE INDEX chunks_domain ON chunks (domain, at_ns);
CREATE INDEX chunks_app    ON chunks (app_id);

CREATE VIRTUAL TABLE chunks_fts USING fts5 (
    text,
    content = 'chunks',
    content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER chunks_ai AFTER INSERT ON chunks BEGIN
    INSERT INTO chunks_fts (rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER chunks_ad AFTER DELETE ON chunks BEGIN
    INSERT INTO chunks_fts (chunks_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
CREATE TRIGGER chunks_au AFTER UPDATE OF text ON chunks BEGIN
    INSERT INTO chunks_fts (chunks_fts, rowid, text) VALUES ('delete', old.id, old.text);
    INSERT INTO chunks_fts (rowid, text) VALUES (new.id, new.text);
END;

CREATE TABLE health_samples (
    id     INTEGER PRIMARY KEY,
    metric TEXT    NOT NULL,
    value  REAL    NOT NULL,
    at_ns  INTEGER NOT NULL,
    rollup INTEGER NOT NULL DEFAULT 0,   -- 0 raw sample, 1 hourly summary
    vmin   REAL    NOT NULL,
    vmax   REAL    NOT NULL,
    n      INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX health_samples_metric ON health_samples (metric, at_ns);
CREATE INDEX health_samples_time   ON health_samples (at_ns);

CREATE TABLE profile_facts (
    id           INTEGER PRIMARY KEY,
    text         TEXT    NOT NULL,
    category     TEXT    NOT NULL DEFAULT '',
    source       TEXT    NOT NULL DEFAULT '',
    created_ns   INTEGER NOT NULL,
    last_used_ns INTEGER NOT NULL,
    taint        INTEGER NOT NULL CHECK (taint BETWEEN 0 AND 15)
);

CREATE TABLE nudge_log (
    id      INTEGER PRIMARY KEY,
    rule_id TEXT    NOT NULL,
    outcome INTEGER NOT NULL,
    at_ns   INTEGER NOT NULL
);
CREATE INDEX nudge_log_time ON nudge_log (at_ns);
