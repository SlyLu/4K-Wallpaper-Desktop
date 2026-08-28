-- New no-key providers participate in the same local-first aggregated search pipeline.
INSERT OR IGNORE INTO provider_config(provider, enabled, updated_at) VALUES
    ('openverse', 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('art_institute_chicago', 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

INSERT OR IGNORE INTO provider_health(provider, updated_at)
SELECT provider, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM provider_config
WHERE provider IN ('openverse', 'art_institute_chicago');

INSERT OR IGNORE INTO provider_sync_state(provider, updated_at)
SELECT provider, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM provider_config
WHERE provider IN ('openverse', 'art_institute_chicago');

-- Search associations are intentionally separate from semantic provider tags. A provider result
-- may match a translated query even when that query does not appear in the image metadata.
CREATE TABLE wallpaper_search_result (
    query TEXT NOT NULL COLLATE NOCASE,
    wallpaper_id INTEGER NOT NULL REFERENCES wallpaper(id) ON DELETE CASCADE,
    matched_at TEXT NOT NULL,
    PRIMARY KEY(query, wallpaper_id)
);

CREATE INDEX wallpaper_search_result_wallpaper_index
    ON wallpaper_search_result(wallpaper_id, matched_at DESC);

-- Earlier builds appended every user query to every remote result. Clear only regenerable tags
-- on unprotected remote metadata; user files and user-protected records are never touched.
DELETE FROM wallpaper_tag
WHERE wallpaper_id IN (
    SELECT wallpaper.id
    FROM wallpaper
    WHERE wallpaper.provider <> 'local'
      AND wallpaper.local_path IS NULL
      AND wallpaper.favorite = 0
      AND wallpaper.preset = 0
      AND NOT EXISTS (
          SELECT 1 FROM rotation_selection
          WHERE rotation_selection.wallpaper_id = wallpaper.id
      )
      AND NOT EXISTS (
          SELECT 1 FROM rotation_wallpaper
          WHERE rotation_wallpaper.wallpaper_id = wallpaper.id
      )
      AND NOT EXISTS (
          SELECT 1 FROM collection_wallpaper
          WHERE collection_wallpaper.wallpaper_id = wallpaper.id
      )
      AND NOT EXISTS (
          SELECT 1 FROM wallpaper_history
          WHERE wallpaper_history.wallpaper_id = wallpaper.id
      )
);
