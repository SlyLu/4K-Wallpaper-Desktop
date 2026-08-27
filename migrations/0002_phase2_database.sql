CREATE TABLE wallpaper (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider TEXT NOT NULL,
    remote_id TEXT NOT NULL,
    name TEXT NOT NULL,
    source_page_url TEXT,
    original_url TEXT,
    thumbnail_url TEXT,
    thumbnail_local_path TEXT,
    local_path TEXT,
    width INTEGER NOT NULL DEFAULT 0 CHECK (width >= 0),
    height INTEGER NOT NULL DEFAULT 0 CHECK (height >= 0),
    aspect_ratio TEXT,
    file_size INTEGER CHECK (file_size IS NULL OR file_size >= 0),
    mime_type TEXT,
    category TEXT NOT NULL,
    purity TEXT NOT NULL DEFAULT 'sfw',
    hash TEXT,
    download_status TEXT NOT NULL DEFAULT 'remote',
    favorite INTEGER NOT NULL DEFAULT 0 CHECK (favorite IN (0, 1)),
    blacklisted INTEGER NOT NULL DEFAULT 0 CHECK (blacklisted IN (0, 1)),
    preset INTEGER NOT NULL DEFAULT 0 CHECK (preset IN (0, 1)),
    created_at TEXT,
    synced_at TEXT NOT NULL,
    downloaded_at TEXT,
    last_used_at TEXT,
    UNIQUE(provider, remote_id)
);

CREATE UNIQUE INDEX wallpaper_hash_unique
    ON wallpaper(hash)
    WHERE hash IS NOT NULL;
CREATE INDEX wallpaper_list_index
    ON wallpaper(blacklisted, category, synced_at DESC, id DESC);

CREATE TABLE tag (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL COLLATE NOCASE UNIQUE
);

CREATE TABLE wallpaper_tag (
    wallpaper_id INTEGER NOT NULL REFERENCES wallpaper(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tag(id) ON DELETE CASCADE,
    PRIMARY KEY(wallpaper_id, tag_id)
);

CREATE TABLE monitor (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    system_monitor_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    width INTEGER NOT NULL CHECK (width > 0),
    height INTEGER NOT NULL CHECK (height > 0),
    position_x INTEGER NOT NULL,
    position_y INTEGER NOT NULL,
    primary_display INTEGER NOT NULL CHECK (primary_display IN (0, 1)),
    last_seen_at TEXT NOT NULL
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE wallpaper_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    wallpaper_id INTEGER NOT NULL REFERENCES wallpaper(id) ON DELETE RESTRICT,
    monitor_id INTEGER REFERENCES monitor(id) ON DELETE SET NULL,
    used_at TEXT NOT NULL,
    trigger_type TEXT NOT NULL CHECK (trigger_type IN ('MANUAL', 'SCHEDULE'))
);

CREATE INDEX wallpaper_history_recent_index
    ON wallpaper_history(used_at DESC, wallpaper_id);
