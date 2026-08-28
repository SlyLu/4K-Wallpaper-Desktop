CREATE TABLE provider_config (
    provider TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    settings_json TEXT NOT NULL DEFAULT '{}',
    updated_at TEXT NOT NULL
);

INSERT INTO provider_config(provider, enabled, updated_at) VALUES
    ('wallhaven', 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('wikimedia_commons', 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('local', 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

CREATE TABLE provider_health (
    provider TEXT PRIMARY KEY REFERENCES provider_config(provider) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'unknown' CHECK (status IN ('unknown', 'healthy', 'degraded', 'unavailable')),
    last_success_at TEXT,
    last_error_at TEXT,
    last_error TEXT,
    response_time_ms INTEGER,
    updated_at TEXT NOT NULL
);

INSERT INTO provider_health(provider, updated_at)
SELECT provider, strftime('%Y-%m-%dT%H:%M:%fZ', 'now') FROM provider_config;

CREATE TABLE provider_sync_state (
    provider TEXT PRIMARY KEY REFERENCES provider_config(provider) ON DELETE CASCADE,
    cursor TEXT,
    last_success_at TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE wallpaper_content_identity (
    wallpaper_id INTEGER PRIMARY KEY REFERENCES wallpaper(id) ON DELETE CASCADE,
    canonical_original_url TEXT,
    sha256 TEXT,
    perceptual_hash TEXT,
    width INTEGER NOT NULL DEFAULT 0,
    height INTEGER NOT NULL DEFAULT 0,
    confidence TEXT NOT NULL DEFAULT 'provider' CHECK (confidence IN ('provider', 'url', 'hash', 'perceptual', 'confirmed')),
    updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX wallpaper_identity_url_unique
    ON wallpaper_content_identity(canonical_original_url)
    WHERE canonical_original_url IS NOT NULL;
CREATE INDEX wallpaper_identity_sha256_index
    ON wallpaper_content_identity(sha256)
    WHERE sha256 IS NOT NULL;
CREATE INDEX wallpaper_identity_perceptual_index
    ON wallpaper_content_identity(perceptual_hash)
    WHERE perceptual_hash IS NOT NULL;

CREATE TABLE wallpaper_provider_source (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    wallpaper_id INTEGER NOT NULL REFERENCES wallpaper(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    remote_id TEXT NOT NULL,
    source_page_url TEXT,
    original_url TEXT,
    author TEXT,
    license_name TEXT,
    license_url TEXT,
    width INTEGER,
    height INTEGER,
    file_size INTEGER,
    mime_type TEXT,
    last_seen_at TEXT NOT NULL,
    UNIQUE(provider, remote_id)
);

CREATE INDEX wallpaper_provider_source_wallpaper_index
    ON wallpaper_provider_source(wallpaper_id, provider);
CREATE INDEX wallpaper_provider_source_url_index
    ON wallpaper_provider_source(original_url)
    WHERE original_url IS NOT NULL;

INSERT INTO wallpaper_content_identity(
    wallpaper_id, canonical_original_url, sha256, width, height, confidence, updated_at
)
SELECT id, original_url, hash, width, height,
       CASE WHEN hash IS NOT NULL THEN 'hash' WHEN original_url IS NOT NULL THEN 'url' ELSE 'provider' END,
       strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM wallpaper
WHERE original_url IS NULL
   OR id = (SELECT MIN(candidate.id) FROM wallpaper candidate
            WHERE candidate.original_url = wallpaper.original_url);

-- Historical catalogs may already contain several provider rows for one URL. Keep those
-- rows usable and let the runtime merge them on the next refresh instead of aborting upgrade.
INSERT INTO wallpaper_content_identity(
    wallpaper_id, canonical_original_url, sha256, width, height, confidence, updated_at
)
SELECT id, NULL, hash, width, height,
       CASE WHEN hash IS NOT NULL THEN 'hash' ELSE 'provider' END,
       strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM wallpaper
WHERE original_url IS NOT NULL
  AND id <> (SELECT MIN(candidate.id) FROM wallpaper candidate
             WHERE candidate.original_url = wallpaper.original_url);

INSERT INTO wallpaper_provider_source(
    wallpaper_id, provider, remote_id, source_page_url, original_url,
    width, height, file_size, mime_type, last_seen_at
)
SELECT id, provider, remote_id, source_page_url, original_url,
       width, height, file_size, mime_type,
       strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM wallpaper;
