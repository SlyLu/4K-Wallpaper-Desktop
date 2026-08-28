CREATE TABLE monitor_rotation_policy (
    system_monitor_id TEXT PRIMARY KEY,
    strategy TEXT NOT NULL CHECK (strategy IN ('round_robin', 'shuffle', 'least_recent', 'weighted_random')),
    rules_json TEXT NOT NULL DEFAULT '{}',
    last_reason TEXT,
    updated_at TEXT NOT NULL
);

INSERT INTO monitor_rotation_policy(system_monitor_id, strategy, rules_json, updated_at)
SELECT system_monitor_id,
       CASE WHEN selection_mode = 'random' THEN 'shuffle' ELSE 'round_robin' END,
       '{}',
       strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM monitor_schedule;

CREATE TABLE monitor_rotation_source (
    system_monitor_id TEXT NOT NULL REFERENCES monitor_rotation_policy(system_monitor_id) ON DELETE CASCADE,
    collection_id INTEGER NOT NULL REFERENCES collection(id) ON DELETE CASCADE,
    weight INTEGER NOT NULL DEFAULT 1 CHECK (weight BETWEEN 1 AND 100),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    PRIMARY KEY(system_monitor_id, collection_id)
);

CREATE TABLE rotation_queue (
    system_monitor_id TEXT NOT NULL REFERENCES monitor_rotation_policy(system_monitor_id) ON DELETE CASCADE,
    generation INTEGER NOT NULL,
    wallpaper_id INTEGER NOT NULL REFERENCES wallpaper(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    consumed_at TEXT,
    PRIMARY KEY(system_monitor_id, generation, wallpaper_id)
);

CREATE INDEX rotation_queue_next_index
    ON rotation_queue(system_monitor_id, generation, consumed_at, position);

ALTER TABLE wallpaper_history ADD COLUMN selection_reason TEXT;
