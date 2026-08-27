DROP INDEX IF EXISTS wallpaper_hash_unique;
CREATE INDEX wallpaper_hash_index
    ON wallpaper(hash)
    WHERE hash IS NOT NULL;

CREATE TABLE monitor_schedule (
    system_monitor_id TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    paused INTEGER NOT NULL DEFAULT 0 CHECK (paused IN (0, 1)),
    interval_seconds INTEGER NOT NULL CHECK (interval_seconds >= 60),
    fit_mode TEXT NOT NULL DEFAULT 'fill' CHECK (fit_mode IN ('fill', 'fit', 'center', 'stretch')),
    last_change_at TEXT,
    next_change_at TEXT NOT NULL,
    last_error TEXT,
    updated_at TEXT NOT NULL
);

CREATE INDEX monitor_schedule_due_index
    ON monitor_schedule(enabled, paused, next_change_at);

CREATE TABLE rotation_wallpaper (
    system_monitor_id TEXT NOT NULL,
    wallpaper_id INTEGER NOT NULL REFERENCES wallpaper(id) ON DELETE CASCADE,
    selected_at TEXT NOT NULL,
    PRIMARY KEY(system_monitor_id, wallpaper_id)
);

CREATE INDEX rotation_wallpaper_selection_index
    ON rotation_wallpaper(system_monitor_id, selected_at, wallpaper_id);
CREATE INDEX rotation_wallpaper_reverse_index
    ON rotation_wallpaper(wallpaper_id, system_monitor_id);
