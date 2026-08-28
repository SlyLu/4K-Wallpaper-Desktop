CREATE TABLE monitor_layout_snapshot (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    layout_hash TEXT NOT NULL UNIQUE,
    layout_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE spanning_assignment (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    wallpaper_id INTEGER REFERENCES wallpaper(id) ON DELETE SET NULL,
    layout_snapshot_id INTEGER REFERENCES monitor_layout_snapshot(id) ON DELETE SET NULL,
    fit_mode TEXT NOT NULL CHECK (fit_mode IN ('fill', 'fit_to_span')),
    previous_paths_json TEXT NOT NULL DEFAULT '[]',
    active INTEGER NOT NULL DEFAULT 0 CHECK (active IN (0, 1)),
    last_error TEXT,
    updated_at TEXT NOT NULL
);
