-- Phase 0 only establishes database identity; V1 domain tables are introduced in Phase 2.
CREATE TABLE app_metadata (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

INSERT INTO app_metadata(key, value) VALUES ('application', '4K Wallpaper Desktop');
