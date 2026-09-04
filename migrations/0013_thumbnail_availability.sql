-- Failed previews are recoverable health state, never user dislikes or deleted resources.
ALTER TABLE wallpaper ADD COLUMN thumbnail_failed INTEGER NOT NULL DEFAULT 0 CHECK (thumbnail_failed IN (0, 1));
