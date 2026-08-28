CREATE TABLE collection (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL COLLATE NOCASE UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    cover_wallpaper_id INTEGER REFERENCES wallpaper(id) ON DELETE SET NULL,
    position INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX collection_position_index ON collection(position, name);

CREATE TABLE collection_wallpaper (
    collection_id INTEGER NOT NULL REFERENCES collection(id) ON DELETE CASCADE,
    wallpaper_id INTEGER NOT NULL REFERENCES wallpaper(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    added_at TEXT NOT NULL,
    PRIMARY KEY(collection_id, wallpaper_id)
);

CREATE INDEX collection_wallpaper_order_index
    ON collection_wallpaper(collection_id, position, added_at, wallpaper_id);
CREATE INDEX collection_wallpaper_reverse_index
    ON collection_wallpaper(wallpaper_id, collection_id);

CREATE TABLE smart_collection_rule (
    collection_id INTEGER PRIMARY KEY REFERENCES collection(id) ON DELETE CASCADE,
    version INTEGER NOT NULL CHECK (version >= 1),
    rule_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
