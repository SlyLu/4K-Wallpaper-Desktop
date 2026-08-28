CREATE TABLE wallpaper_file_state (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    wallpaper_id INTEGER NOT NULL REFERENCES wallpaper(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    storage_kind TEXT NOT NULL CHECK (storage_kind IN ('user_source', 'managed_download', 'processed', 'thumbnail')),
    availability TEXT NOT NULL CHECK (availability IN ('available', 'temporarily_unavailable', 'missing')),
    file_size INTEGER CHECK (file_size IS NULL OR file_size >= 0),
    modified_at_ms INTEGER CHECK (modified_at_ms IS NULL OR modified_at_ms >= 0),
    content_hash TEXT,
    last_verified_at TEXT NOT NULL,
    missing_since TEXT,
    UNIQUE(path)
);
CREATE INDEX wallpaper_file_state_wallpaper_index
    ON wallpaper_file_state(wallpaper_id, availability, storage_kind);
CREATE INDEX wallpaper_file_state_snapshot_index
    ON wallpaper_file_state(path, file_size, modified_at_ms);
CREATE INDEX wallpaper_file_state_hash_index
    ON wallpaper_file_state(content_hash)
    WHERE content_hash IS NOT NULL;

-- Preserve every V1 local-path relationship while moving file lifecycle state into V2.
INSERT INTO wallpaper_file_state(
    wallpaper_id, path, storage_kind, availability, file_size, content_hash, last_verified_at
)
SELECT
    id,
    local_path,
    CASE WHEN provider = 'local' THEN 'user_source' ELSE 'managed_download' END,
    'available',
    file_size,
    hash,
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM wallpaper
WHERE local_path IS NOT NULL;

CREATE TABLE wallpaper_exclusion (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider TEXT,
    remote_id TEXT,
    normalized_path TEXT,
    content_hash TEXT,
    reason TEXT NOT NULL CHECK (reason IN ('blacklisted', 'index_removed')),
    created_at TEXT NOT NULL,
    CHECK (
        remote_id IS NOT NULL OR normalized_path IS NOT NULL OR content_hash IS NOT NULL
    )
);

CREATE UNIQUE INDEX wallpaper_exclusion_provider_remote_index
    ON wallpaper_exclusion(provider, remote_id)
    WHERE provider IS NOT NULL AND remote_id IS NOT NULL;
CREATE UNIQUE INDEX wallpaper_exclusion_path_index
    ON wallpaper_exclusion(normalized_path)
    WHERE normalized_path IS NOT NULL;
CREATE INDEX wallpaper_exclusion_hash_index
    ON wallpaper_exclusion(content_hash)
    WHERE content_hash IS NOT NULL;

INSERT OR IGNORE INTO wallpaper_exclusion(
    provider, remote_id, normalized_path, content_hash, reason, created_at
)
SELECT provider, remote_id, local_path, hash, 'blacklisted', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM wallpaper
WHERE blacklisted = 1;

CREATE TABLE gallery_scan_root (
    path TEXT PRIMARY KEY,
    last_scan_started_at TEXT,
    last_scan_completed_at TEXT,
    last_status TEXT NOT NULL DEFAULT 'pending'
        CHECK (last_status IN ('pending', 'available', 'temporarily_unavailable', 'failed')),
    last_error TEXT
);
