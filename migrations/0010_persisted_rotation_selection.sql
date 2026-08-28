-- The catalog selection is a durable source pool; per-monitor schedules may override it.
CREATE TABLE rotation_selection (
    wallpaper_id INTEGER PRIMARY KEY REFERENCES wallpaper(id) ON DELETE CASCADE,
    selected_at TEXT NOT NULL
);
