use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::Value};

use crate::{
    error::{AppError, AppResult},
    image_processing::ImageMetadata,
    models::{
        CatalogQuery, MonitorInfo, NewWallpaper, ScheduleRecord, WallpaperPage, WallpaperRecord,
    },
};

const MIGRATIONS: &[(i64, &str)] = &[
    (
        1,
        include_str!("../../../migrations/0001_phase0_bootstrap.sql"),
    ),
    (
        2,
        include_str!("../../../migrations/0002_phase2_database.sql"),
    ),
    (
        3,
        include_str!("../../../migrations/0003_phase5_wallpaper_core.sql"),
    ),
    (
        4,
        include_str!("../../../migrations/0004_rotation_selection_modes.sql"),
    ),
];

/// Owns the single local SQLite connection behind a mutex for Tauri command access.
#[derive(Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
}

impl Database {
    /// Opens SQLite, enables integrity pragmas, and applies ordered embedded migrations.
    pub fn open(path: &Path) -> AppResult<Self> {
        let mut connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;",
        )?;
        apply_migrations(&mut connection)?;
        tracing::info!(database = %path.display(), "database initialized");
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    /// Reads the schema version for diagnostics without exposing the raw connection.
    pub fn schema_version(&self) -> AppResult<i64> {
        let connection = self.lock()?;
        Ok(connection.query_row(
            "SELECT version FROM schema_migration ORDER BY version DESC LIMIT 1",
            [],
            |row| row.get(0),
        )?)
    }

    /// Inserts or refreshes provider metadata while preserving user-owned catalog state.
    pub fn upsert_wallpapers(&self, wallpapers: &[NewWallpaper]) -> AppResult<usize> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        for wallpaper in wallpapers {
            transaction.execute(
                "INSERT INTO wallpaper (
                    provider, remote_id, name, source_page_url, original_url, thumbnail_url,
                    thumbnail_local_path, local_path, width, height, aspect_ratio, file_size,
                    mime_type, category, purity, hash, download_status, preset, created_at, synced_at
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                    ?16, ?17, ?18, ?19, ?20
                 ) ON CONFLICT(provider, remote_id) DO UPDATE SET
                    name = excluded.name,
                    source_page_url = excluded.source_page_url,
                    original_url = excluded.original_url,
                    thumbnail_url = excluded.thumbnail_url,
                    thumbnail_local_path = COALESCE(excluded.thumbnail_local_path, wallpaper.thumbnail_local_path),
                    width = excluded.width,
                    height = excluded.height,
                    aspect_ratio = excluded.aspect_ratio,
                    file_size = excluded.file_size,
                    mime_type = excluded.mime_type,
                    category = excluded.category,
                    purity = excluded.purity,
                    preset = MAX(wallpaper.preset, excluded.preset),
                    created_at = excluded.created_at,
                    synced_at = excluded.synced_at",
                params![
                    wallpaper.provider, wallpaper.remote_id, wallpaper.name,
                    wallpaper.source_page_url, wallpaper.original_url, wallpaper.thumbnail_url,
                    wallpaper.thumbnail_local_path, wallpaper.local_path, wallpaper.width,
                    wallpaper.height, wallpaper.aspect_ratio, wallpaper.file_size,
                    wallpaper.mime_type, wallpaper.category, wallpaper.purity, wallpaper.hash,
                    wallpaper.download_status, wallpaper.preset, wallpaper.created_at,
                    wallpaper.synced_at,
                ],
            )?;

            let wallpaper_id: i64 = transaction.query_row(
                "SELECT id FROM wallpaper WHERE provider = ?1 AND remote_id = ?2",
                params![wallpaper.provider, wallpaper.remote_id],
                |row| row.get(0),
            )?;
            for tag in &wallpaper.tags {
                let normalized = tag.trim();
                if normalized.is_empty() {
                    continue;
                }
                transaction.execute(
                    "INSERT INTO tag(name) VALUES (?1) ON CONFLICT(name) DO NOTHING",
                    [normalized],
                )?;
                transaction.execute(
                    "INSERT INTO wallpaper_tag(wallpaper_id, tag_id)
                     SELECT ?1, id FROM tag WHERE name = ?2
                     ON CONFLICT(wallpaper_id, tag_id) DO NOTHING",
                    params![wallpaper_id, normalized],
                )?;
            }
        }
        transaction.commit()?;
        Ok(wallpapers.len())
    }

    /// Returns one bounded page and a total count for stable UI pagination.
    pub fn list_wallpapers(
        &self,
        page: u32,
        page_size: u32,
        preset_only: bool,
    ) -> AppResult<WallpaperPage> {
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let offset = i64::from(page - 1) * i64::from(page_size);
        let connection = self.lock()?;
        let total: i64 = connection.query_row(
            "SELECT COUNT(*) FROM wallpaper WHERE blacklisted = 0 AND (?1 = 0 OR preset = 1)",
            [preset_only],
            |row| row.get(0),
        )?;
        let mut statement = connection.prepare(
            "SELECT id, provider, remote_id, name, source_page_url, original_url, thumbnail_url,
                    thumbnail_local_path, local_path, width, height, aspect_ratio, file_size,
                    mime_type, category, purity, hash, download_status, favorite, blacklisted,
                    preset, created_at, synced_at, downloaded_at, last_used_at
             FROM wallpaper
             WHERE blacklisted = 0 AND (?1 = 0 OR preset = 1)
             ORDER BY preset DESC, synced_at DESC, id DESC
             LIMIT ?2 OFFSET ?3",
        )?;
        let rows = statement.query_map(params![preset_only, page_size, offset], map_wallpaper)?;
        let mut items = Vec::with_capacity(page_size as usize);
        for row in rows {
            let mut wallpaper = row?;
            wallpaper.tags = load_tags(&connection, wallpaper.id)?;
            items.push(wallpaper);
        }
        Ok(WallpaperPage {
            items,
            page,
            page_size,
            total: u64::try_from(total)
                .map_err(|_| AppError::Database("wallpaper count was negative".into()))?,
        })
    }

    /// Searches persisted metadata with parameterized filters shared by all Phase 7 pages.
    pub fn search_wallpapers(&self, query: &CatalogQuery) -> AppResult<WallpaperPage> {
        let page = query.page.max(1);
        let page_size = query.page_size.clamp(1, 100);
        let offset = i64::from(page - 1) * i64::from(page_size);
        let mut predicates = vec![if query.include_blacklisted {
            "1 = 1".to_owned()
        } else {
            "wallpaper.blacklisted = 0".to_owned()
        }];
        let mut values = Vec::<Value>::new();

        if let Some(keyword) = query
            .keyword
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            let slot = values.len() + 1;
            predicates.push(format!(
                "(wallpaper.name LIKE ?{slot} COLLATE NOCASE OR wallpaper.provider LIKE ?{slot} COLLATE NOCASE OR wallpaper.category LIKE ?{slot} COLLATE NOCASE OR EXISTS (SELECT 1 FROM wallpaper_tag wt JOIN tag t ON t.id = wt.tag_id WHERE wt.wallpaper_id = wallpaper.id AND t.name LIKE ?{slot} COLLATE NOCASE))"
            ));
            values.push(Value::Text(format!("%{keyword}%")));
        }
        if let Some(name) = query
            .name
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            let slot = values.len() + 1;
            predicates.push(format!("wallpaper.name LIKE ?{slot} COLLATE NOCASE"));
            values.push(Value::Text(format!("%{name}%")));
        }
        if let Some(category) = query
            .category
            .as_deref()
            .filter(|value| !value.eq_ignore_ascii_case("all"))
        {
            let slot = values.len() + 1;
            predicates.push(format!("wallpaper.category = ?{slot} COLLATE NOCASE"));
            values.push(Value::Text(category.to_owned()));
        }
        if let Some(provider) = query
            .provider
            .as_deref()
            .filter(|value| !value.eq_ignore_ascii_case("all"))
        {
            let slot = values.len() + 1;
            predicates.push(format!("wallpaper.provider = ?{slot} COLLATE NOCASE"));
            values.push(Value::Text(provider.to_owned()));
        }
        if query.locally_available {
            // Provider identity is retained: a downloaded Wallhaven item is locally available,
            // but it must not be rewritten as a LocalProvider-owned user file.
            predicates.push(
                "wallpaper.local_path IS NOT NULL AND wallpaper.download_status = 'downloaded'"
                    .to_owned(),
            );
        }
        if let Some(favorite) = query.favorite {
            let slot = values.len() + 1;
            predicates.push(format!("wallpaper.favorite = ?{slot}"));
            values.push(Value::Integer(i64::from(favorite)));
        }
        if let Some(min_width) = query.min_width.filter(|value| *value > 0) {
            let slot = values.len() + 1;
            predicates.push(format!("wallpaper.width >= ?{slot}"));
            values.push(Value::Integer(i64::from(min_width)));
        }
        if let Some(min_height) = query.min_height.filter(|value| *value > 0) {
            let slot = values.len() + 1;
            predicates.push(format!("wallpaper.height >= ?{slot}"));
            values.push(Value::Integer(i64::from(min_height)));
        }

        let where_clause = predicates.join(" AND ");
        let order_clause = match query.sort.as_deref() {
            Some("random") => "RANDOM()",
            Some("name") => "wallpaper.name COLLATE NOCASE ASC",
            _ => "wallpaper.synced_at DESC, wallpaper.id DESC",
        };
        let connection = self.lock()?;
        let total_sql = format!("SELECT COUNT(*) FROM wallpaper WHERE {where_clause}");
        let total: i64 =
            connection.query_row(&total_sql, params_from_iter(values.iter()), |row| {
                row.get(0)
            })?;
        let limit_slot = values.len() + 1;
        let offset_slot = values.len() + 2;
        let sql = format!(
            "SELECT id, provider, remote_id, name, source_page_url, original_url, thumbnail_url,
                    thumbnail_local_path, local_path, width, height, aspect_ratio, file_size,
                    mime_type, category, purity, hash, download_status, favorite, blacklisted,
                    preset, created_at, synced_at, downloaded_at, last_used_at
             FROM wallpaper WHERE {where_clause} ORDER BY {order_clause}
             LIMIT ?{limit_slot} OFFSET ?{offset_slot}"
        );
        let mut page_values = values;
        page_values.push(Value::Integer(i64::from(page_size)));
        page_values.push(Value::Integer(offset));
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(page_values.iter()), map_wallpaper)?;
        let mut items = Vec::with_capacity(page_size as usize);
        for row in rows {
            let mut wallpaper = row?;
            wallpaper.tags = load_tags(&connection, wallpaper.id)?;
            items.push(wallpaper);
        }
        Ok(WallpaperPage {
            items,
            page,
            page_size,
            total: u64::try_from(total)
                .map_err(|_| AppError::Database("wallpaper count was negative".into()))?,
        })
    }

    /// Loads one catalog item with tags for detail, download, and scheduler operations.
    pub fn get_wallpaper(&self, wallpaper_id: i64) -> AppResult<WallpaperRecord> {
        let connection = self.lock()?;
        load_wallpaper(&connection, wallpaper_id)?
            .ok_or_else(|| AppError::Wallpaper(format!("wallpaper does not exist: {wallpaper_id}")))
    }

    /// Finds an already-downloaded file with identical content before retaining a new copy.
    pub fn downloaded_path_by_hash(
        &self,
        sha256: &str,
        excluding_wallpaper_id: i64,
    ) -> AppResult<Option<String>> {
        let connection = self.lock()?;
        Ok(connection
            .query_row(
                "SELECT local_path FROM wallpaper
                 WHERE hash = ?1 AND id <> ?2 AND local_path IS NOT NULL
                 LIMIT 1",
                params![sha256, excluding_wallpaper_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Commits verified local metadata only after the original has been fully decoded.
    pub fn mark_wallpaper_downloaded(
        &self,
        wallpaper_id: i64,
        local_path: &Path,
        metadata: &ImageMetadata,
    ) -> AppResult<WallpaperRecord> {
        let file_size = i64::try_from(metadata.file_size)
            .map_err(|_| AppError::Database("downloaded image size exceeds SQLite range".into()))?;
        let connection = self.lock()?;
        let updated = connection.execute(
            "UPDATE wallpaper SET
                local_path = ?2,
                width = ?3,
                height = ?4,
                aspect_ratio = ?5,
                file_size = ?6,
                mime_type = ?7,
                hash = ?8,
                download_status = 'downloaded',
                downloaded_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1",
            params![
                wallpaper_id,
                local_path.display().to_string(),
                metadata.width,
                metadata.height,
                metadata.aspect_ratio,
                file_size,
                metadata.mime_type,
                metadata.sha256,
            ],
        )?;
        if updated != 1 {
            return Err(AppError::Wallpaper(format!(
                "wallpaper does not exist: {wallpaper_id}"
            )));
        }
        drop(connection);
        self.get_wallpaper(wallpaper_id)
    }

    /// Updates favorite state and returns the refreshed catalog item.
    pub fn set_wallpaper_favorite(
        &self,
        wallpaper_id: i64,
        favorite: bool,
    ) -> AppResult<WallpaperRecord> {
        let connection = self.lock()?;
        let updated = connection.execute(
            "UPDATE wallpaper SET favorite = ?2 WHERE id = ?1",
            params![wallpaper_id, favorite],
        )?;
        if updated != 1 {
            return Err(AppError::Wallpaper(format!(
                "wallpaper does not exist: {wallpaper_id}"
            )));
        }
        drop(connection);
        self.get_wallpaper(wallpaper_id)
    }

    /// Blacklisting also removes the item from every rotation pool in one transaction.
    pub fn set_wallpaper_blacklisted(
        &self,
        wallpaper_id: i64,
        blacklisted: bool,
    ) -> AppResult<WallpaperRecord> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE wallpaper SET blacklisted = ?2 WHERE id = ?1",
            params![wallpaper_id, blacklisted],
        )?;
        if updated != 1 {
            return Err(AppError::Wallpaper(format!(
                "wallpaper does not exist: {wallpaper_id}"
            )));
        }
        if blacklisted {
            transaction.execute(
                "DELETE FROM rotation_wallpaper WHERE wallpaper_id = ?1",
                [wallpaper_id],
            )?;
        }
        transaction.commit()?;
        drop(connection);
        self.get_wallpaper(wallpaper_id)
    }

    /// Clears one remote original reference after the command layer has validated ownership.
    pub fn clear_wallpaper_download(&self, wallpaper_id: i64) -> AppResult<WallpaperRecord> {
        let connection = self.lock()?;
        let changed = connection.execute(
            "UPDATE wallpaper SET local_path = NULL, hash = NULL, download_status = 'remote', downloaded_at = NULL WHERE id = ?1 AND provider <> 'local'",
            [wallpaper_id],
        )?;
        drop(connection);
        if changed != 1 {
            return Err(AppError::Wallpaper(
                "only downloaded remote wallpaper cache can be removed".into(),
            ));
        }
        self.get_wallpaper(wallpaper_id)
    }

    /// Removes a LocalProvider index and dependent history without touching the user-owned file.
    pub fn remove_local_wallpaper_index(&self, wallpaper_id: i64) -> AppResult<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let provider = transaction
            .query_row(
                "SELECT provider FROM wallpaper WHERE id = ?1",
                [wallpaper_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| AppError::Wallpaper("wallpaper does not exist".into()))?;
        if provider != "local" {
            return Err(AppError::Wallpaper(
                "only LocalProvider indexes can be removed by this operation".into(),
            ));
        }
        transaction.execute(
            "DELETE FROM wallpaper_history WHERE wallpaper_id = ?1",
            [wallpaper_id],
        )?;
        transaction.execute("DELETE FROM wallpaper WHERE id = ?1", [wallpaper_id])?;
        transaction.commit()?;
        Ok(())
    }

    /// Prunes indexes whose LocalProvider source files were deleted outside the application.
    pub fn prune_missing_local_wallpapers(&self) -> AppResult<usize> {
        let mut connection = self.lock()?;
        let missing_ids = {
            let mut statement = connection.prepare(
                "SELECT id, local_path FROM wallpaper
                 WHERE provider = 'local' AND local_path IS NOT NULL",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut ids = Vec::new();
            for row in rows {
                let (id, path) = row?;
                if !Path::new(&path).is_file() {
                    ids.push(id);
                }
            }
            ids
        };
        let transaction = connection.transaction()?;
        for wallpaper_id in &missing_ids {
            transaction.execute(
                "DELETE FROM wallpaper_history WHERE wallpaper_id = ?1",
                [wallpaper_id],
            )?;
            transaction.execute("DELETE FROM wallpaper WHERE id = ?1", [wallpaper_id])?;
        }
        transaction.commit()?;
        Ok(missing_ids.len())
    }

    /// Counts other metadata rows sharing a content-deduplicated original path.
    pub fn other_path_references(&self, wallpaper_id: i64, path: &str) -> AppResult<u64> {
        let connection = self.lock()?;
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM wallpaper WHERE id <> ?1 AND local_path = ?2",
            params![wallpaper_id, path],
            |row| row.get(0),
        )?;
        u64::try_from(count)
            .map_err(|_| AppError::Database("path reference count was negative".into()))
    }

    /// Lists distinct non-favorite remote originals from least to most recently used.
    pub fn cache_original_candidates(&self) -> AppResult<Vec<String>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT local_path FROM wallpaper
             WHERE provider <> 'local' AND local_path IS NOT NULL
             GROUP BY local_path
             HAVING MAX(favorite) = 0
             ORDER BY MIN(COALESCE(last_used_at, downloaded_at, synced_at)) ASC",
        )?;
        let rows = statement.query_map([], |row| row.get(0))?;
        let mut paths = Vec::new();
        for row in rows {
            paths.push(row?);
        }
        Ok(paths)
    }

    /// Clears every metadata reference after one deduplicated original file is removed.
    pub fn clear_downloaded_path(&self, path: &str) -> AppResult<usize> {
        let connection = self.lock()?;
        Ok(connection.execute(
            "UPDATE wallpaper SET local_path = NULL, hash = NULL, download_status = 'remote', downloaded_at = NULL
             WHERE provider <> 'local' AND local_path = ?1 AND favorite = 0",
            [path],
        )?)
    }

    /// Clears thumbnail references after an application-owned cache file is removed.
    pub fn clear_thumbnail_path(&self, path: &str) -> AppResult<usize> {
        let connection = self.lock()?;
        Ok(connection.execute(
            "UPDATE wallpaper SET thumbnail_local_path = NULL WHERE thumbnail_local_path = ?1",
            [path],
        )?)
    }

    /// Refreshes active monitor geometry while retaining rows for disconnected displays.
    pub fn upsert_monitors(&self, monitors: &[MonitorInfo]) -> AppResult<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        for monitor in monitors {
            transaction.execute(
                "INSERT INTO monitor (
                    system_monitor_id, name, width, height, position_x, position_y,
                    primary_display, last_seen_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                 ON CONFLICT(system_monitor_id) DO UPDATE SET
                    name = excluded.name,
                    width = excluded.width,
                    height = excluded.height,
                    position_x = excluded.position_x,
                    position_y = excluded.position_y,
                    primary_display = excluded.primary_display,
                    last_seen_at = excluded.last_seen_at",
                params![
                    monitor.system_monitor_id,
                    monitor.name,
                    monitor.width,
                    monitor.height,
                    monitor.position_x,
                    monitor.position_y,
                    monitor.primary,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Replaces one monitor's selected rotation pool and schedules an immediate first run.
    pub fn configure_rotation(
        &self,
        system_monitor_id: &str,
        wallpaper_ids: &[i64],
        interval_seconds: u64,
        fit_mode: &str,
        selection_mode: &str,
    ) -> AppResult<ScheduleRecord> {
        if interval_seconds < 60 {
            return Err(AppError::Configuration(
                "rotation interval must be at least 60 seconds".into(),
            ));
        }
        if !matches!(fit_mode, "fill" | "fit" | "center" | "stretch") {
            return Err(AppError::Configuration(format!(
                "unsupported fit mode: {fit_mode}"
            )));
        }
        if !matches!(selection_mode, "round_robin" | "random") {
            return Err(AppError::Configuration(format!(
                "unsupported rotation selection mode: {selection_mode}"
            )));
        }
        let interval = i64::try_from(interval_seconds)
            .map_err(|_| AppError::Configuration("rotation interval is too large".into()))?;
        let mut unique_ids = wallpaper_ids.to_vec();
        unique_ids.sort_unstable();
        unique_ids.dedup();
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        if !unique_ids.is_empty() {
            let placeholders = std::iter::repeat_n("?", unique_ids.len())
                .collect::<Vec<_>>()
                .join(",");
            let valid_count: i64 = transaction.query_row(
                &format!(
                    "SELECT COUNT(*) FROM wallpaper
                     WHERE blacklisted = 0 AND id IN ({placeholders})"
                ),
                rusqlite::params_from_iter(unique_ids.iter()),
                |row| row.get(0),
            )?;
            if usize::try_from(valid_count).ok() != Some(unique_ids.len()) {
                return Err(AppError::Wallpaper(
                    "rotation contains missing or blacklisted wallpaper".into(),
                ));
            }
        }
        transaction.execute(
            "INSERT INTO monitor_schedule (
                system_monitor_id, enabled, paused, interval_seconds, fit_mode, selection_mode,
                next_change_at, updated_at
             ) VALUES (?1, 1, 0, ?2, ?3, ?4,
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(system_monitor_id) DO UPDATE SET
                enabled = 1,
                paused = 0,
                interval_seconds = excluded.interval_seconds,
                fit_mode = excluded.fit_mode,
                selection_mode = excluded.selection_mode,
                next_change_at = excluded.next_change_at,
                last_error = NULL,
                updated_at = excluded.updated_at",
            params![system_monitor_id, interval, fit_mode, selection_mode],
        )?;
        transaction.execute(
            "DELETE FROM rotation_wallpaper WHERE system_monitor_id = ?1",
            [system_monitor_id],
        )?;
        for wallpaper_id in unique_ids {
            transaction.execute(
                "INSERT INTO rotation_wallpaper(system_monitor_id, wallpaper_id, selected_at)
                 VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                params![system_monitor_id, wallpaper_id],
            )?;
        }
        transaction.commit()?;
        drop(connection);
        self.schedule_for_monitor(system_monitor_id)?
            .ok_or_else(|| AppError::Database("configured schedule could not be reloaded".into()))
    }

    /// Lists persisted scheduler state for UI and restart recovery diagnostics.
    pub fn list_schedules(&self) -> AppResult<Vec<ScheduleRecord>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT schedule.system_monitor_id, schedule.enabled, schedule.paused,
                    schedule.interval_seconds, schedule.fit_mode, schedule.last_change_at,
                    schedule.next_change_at, schedule.last_error,
                    COUNT(rotation.wallpaper_id), schedule.selection_mode
             FROM monitor_schedule AS schedule
             LEFT JOIN rotation_wallpaper AS rotation
               ON rotation.system_monitor_id = schedule.system_monitor_id
             GROUP BY schedule.system_monitor_id
             ORDER BY schedule.system_monitor_id",
        )?;
        let rows = statement.query_map([], map_schedule)?;
        let mut schedules = Vec::new();
        for row in rows {
            schedules.push(row?);
        }
        Ok(schedules)
    }

    /// Returns only enabled, unpaused, due schedules through the dedicated due index.
    pub fn due_schedules(&self) -> AppResult<Vec<ScheduleRecord>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT schedule.system_monitor_id, schedule.enabled, schedule.paused,
                    schedule.interval_seconds, schedule.fit_mode, schedule.last_change_at,
                    schedule.next_change_at, schedule.last_error,
                    (SELECT COUNT(*) FROM rotation_wallpaper AS rotation
                     WHERE rotation.system_monitor_id = schedule.system_monitor_id),
                    schedule.selection_mode
             FROM monitor_schedule AS schedule
             WHERE schedule.enabled = 1 AND schedule.paused = 0
               AND schedule.next_change_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             ORDER BY schedule.next_change_at",
        )?;
        let rows = statement.query_map([], map_schedule)?;
        let mut schedules = Vec::new();
        for row in rows {
            schedules.push(row?);
        }
        Ok(schedules)
    }

    /// Selects from the explicit pool, or falls back to five recent valid local originals.
    pub fn next_rotation_wallpaper(&self, system_monitor_id: &str) -> AppResult<WallpaperRecord> {
        let connection = self.lock()?;
        let schedule = connection
            .query_row(
                "SELECT selection_mode,
                        (SELECT COUNT(*) FROM rotation_wallpaper
                         WHERE system_monitor_id = monitor_schedule.system_monitor_id)
                 FROM monitor_schedule WHERE system_monitor_id = ?1 AND enabled = 1",
                [system_monitor_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or_else(|| AppError::Configuration("enabled schedule does not exist".into()))?;

        if schedule.1 > 0 {
            let order = if schedule.0 == "random" {
                "CASE WHEN wallpaper.id = COALESCE((
                    SELECT history.wallpaper_id FROM wallpaper_history AS history
                    JOIN monitor ON monitor.id = history.monitor_id
                    WHERE monitor.system_monitor_id = ?1
                    ORDER BY history.used_at DESC, history.id DESC LIMIT 1
                 ), -1) THEN 1 ELSE 0 END, RANDOM()"
            } else {
                "COALESCE((
                    SELECT MAX(history.used_at) FROM wallpaper_history AS history
                    JOIN monitor ON monitor.id = history.monitor_id
                    WHERE history.wallpaper_id = wallpaper.id
                      AND monitor.system_monitor_id = ?1
                 ), '') ASC, rotation.selected_at ASC, wallpaper.id ASC"
            };
            let wallpaper_id = connection
                .query_row(
                    &format!(
                        "SELECT wallpaper.id
                         FROM rotation_wallpaper AS rotation
                         JOIN wallpaper ON wallpaper.id = rotation.wallpaper_id
                         WHERE rotation.system_monitor_id = ?1 AND wallpaper.blacklisted = 0
                         ORDER BY {order} LIMIT 1"
                    ),
                    [system_monitor_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .ok_or_else(|| {
                    AppError::Wallpaper("selected rotation pool has no usable wallpaper".into())
                })?;
            return load_wallpaper(&connection, wallpaper_id)?.ok_or_else(|| {
                AppError::Wallpaper("selected rotation wallpaper disappeared".into())
            });
        }

        let order = if schedule.0 == "random" {
            "CASE WHEN recent.used_at = (SELECT MAX(used_at) FROM recent) THEN 1 ELSE 0 END,
             RANDOM()"
        } else {
            "recent.used_at ASC, wallpaper.id ASC"
        };
        let sql = format!(
            "WITH recent AS (
                SELECT history.wallpaper_id, MAX(history.used_at) AS used_at
                FROM wallpaper_history AS history
                JOIN monitor ON monitor.id = history.monitor_id
                JOIN wallpaper ON wallpaper.id = history.wallpaper_id
                WHERE monitor.system_monitor_id = ?1
                  AND wallpaper.blacklisted = 0
                  AND wallpaper.local_path IS NOT NULL
                  AND wallpaper.download_status = 'downloaded'
                GROUP BY history.wallpaper_id
                ORDER BY used_at DESC
                LIMIT 5
             )
             SELECT wallpaper.id FROM recent
             JOIN wallpaper ON wallpaper.id = recent.wallpaper_id
             ORDER BY {order}"
        );
        let candidate_ids = {
            let mut statement = connection.prepare(&sql)?;
            let rows = statement.query_map([system_monitor_id], |row| row.get::<_, i64>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for wallpaper_id in candidate_ids {
            if let Some(wallpaper) = load_wallpaper(&connection, wallpaper_id)?
                && wallpaper
                    .local_path
                    .as_deref()
                    .is_some_and(|path| Path::new(path).is_file())
            {
                return Ok(wallpaper);
            }
        }
        Err(AppError::Wallpaper(
            "no usable wallpaper in the five most recent assignments".into(),
        ))
    }

    /// Records successful use and updates the next run relative to now, preventing catch-up loops.
    pub fn complete_schedule_run(
        &self,
        system_monitor_id: &str,
        wallpaper_id: Option<i64>,
        error: Option<&str>,
    ) -> AppResult<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        if let Some(wallpaper_id) = wallpaper_id {
            let monitor_id: Option<i64> = transaction
                .query_row(
                    "SELECT id FROM monitor WHERE system_monitor_id = ?1",
                    [system_monitor_id],
                    |row| row.get(0),
                )
                .optional()?;
            transaction.execute(
                "INSERT INTO wallpaper_history(wallpaper_id, monitor_id, used_at, trigger_type)
                 VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), 'SCHEDULE')",
                params![wallpaper_id, monitor_id],
            )?;
            transaction.execute(
                "UPDATE wallpaper
                 SET last_used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1",
                [wallpaper_id],
            )?;
        }
        transaction.execute(
            "UPDATE monitor_schedule SET
                last_change_at = CASE WHEN ?2 IS NULL
                    THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') ELSE last_change_at END,
                next_change_at = strftime(
                    '%Y-%m-%dT%H:%M:%fZ', 'now', '+' || interval_seconds || ' seconds'
                ),
                last_error = ?2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE system_monitor_id = ?1",
            params![system_monitor_id, error],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Records a successful user-triggered wallpaper change for repeat avoidance and diagnostics.
    pub fn record_manual_history(
        &self,
        wallpaper_id: i64,
        system_monitor_id: &str,
    ) -> AppResult<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let monitor_id: Option<i64> = transaction
            .query_row(
                "SELECT id FROM monitor WHERE system_monitor_id = ?1",
                [system_monitor_id],
                |row| row.get(0),
            )
            .optional()?;
        transaction.execute(
            "INSERT INTO wallpaper_history(wallpaper_id, monitor_id, used_at, trigger_type)
             VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), 'MANUAL')",
            params![wallpaper_id, monitor_id],
        )?;
        transaction.execute(
            "UPDATE wallpaper
             SET last_used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1",
            [wallpaper_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Pauses or resumes one monitor; resume requests one immediate run.
    pub fn set_schedule_paused(
        &self,
        system_monitor_id: &str,
        paused: bool,
    ) -> AppResult<ScheduleRecord> {
        let connection = self.lock()?;
        let updated = connection.execute(
            "UPDATE monitor_schedule SET
                paused = ?2,
                next_change_at = CASE WHEN ?2 = 0
                    THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') ELSE next_change_at END,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE system_monitor_id = ?1",
            params![system_monitor_id, paused],
        )?;
        if updated != 1 {
            return Err(AppError::Configuration(
                "schedule does not exist for selected monitor".into(),
            ));
        }
        drop(connection);
        self.schedule_for_monitor(system_monitor_id)?
            .ok_or_else(|| AppError::Database("updated schedule could not be reloaded".into()))
    }

    /// Marks one schedule due now for the explicit Next action.
    pub fn trigger_schedule_now(&self, system_monitor_id: &str) -> AppResult<()> {
        let connection = self.lock()?;
        let updated = connection.execute(
            "UPDATE monitor_schedule SET
                paused = 0,
                next_change_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE system_monitor_id = ?1 AND enabled = 1",
            [system_monitor_id],
        )?;
        if updated != 1 {
            return Err(AppError::Configuration(
                "enabled schedule does not exist for selected monitor".into(),
            ));
        }
        Ok(())
    }

    /// Reads a single schedule with its selected item count.
    fn schedule_for_monitor(&self, system_monitor_id: &str) -> AppResult<Option<ScheduleRecord>> {
        let connection = self.lock()?;
        Ok(connection
            .query_row(
                "SELECT schedule.system_monitor_id, schedule.enabled, schedule.paused,
                        schedule.interval_seconds, schedule.fit_mode, schedule.last_change_at,
                        schedule.next_change_at, schedule.last_error,
                        (SELECT COUNT(*) FROM rotation_wallpaper AS rotation
                         WHERE rotation.system_monitor_id = schedule.system_monitor_id),
                        schedule.selection_mode
                 FROM monitor_schedule AS schedule
                 WHERE schedule.system_monitor_id = ?1",
                [system_monitor_id],
                map_schedule,
            )
            .optional()?)
    }

    /// Stores a JSON setting atomically under a stable key.
    pub fn set_setting(&self, key: &str, value: &serde_json::Value) -> AppResult<()> {
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO settings(key, value_json, updated_at)
             VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(key) DO UPDATE SET
                value_json = excluded.value_json,
                updated_at = excluded.updated_at",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }

    /// Loads an optional JSON setting without assigning product defaults in persistence code.
    pub fn get_setting(&self, key: &str) -> AppResult<Option<serde_json::Value>> {
        let connection = self.lock()?;
        let value = connection
            .query_row(
                "SELECT value_json FROM settings WHERE key = ?1",
                [key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        value
            .map(|json| serde_json::from_str(&json).map_err(AppError::from))
            .transpose()
    }

    /// Returns only the cached thumbnail path needed by the binary thumbnail command.
    pub fn thumbnail_path(&self, wallpaper_id: i64) -> AppResult<Option<String>> {
        let connection = self.lock()?;
        Ok(connection
            .query_row(
                "SELECT thumbnail_local_path FROM wallpaper WHERE id = ?1 AND blacklisted = 0",
                [wallpaper_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten())
    }

    /// Locks the connection and converts poisoning into a recoverable database error.
    fn lock(&self) -> AppResult<std::sync::MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| AppError::Database("database mutex was poisoned".into()))
    }
}

/// Converts a catalog row without performing any follow-up queries.
fn map_wallpaper(row: &Row<'_>) -> rusqlite::Result<WallpaperRecord> {
    Ok(WallpaperRecord {
        id: row.get(0)?,
        provider: row.get(1)?,
        remote_id: row.get(2)?,
        name: row.get(3)?,
        source_page_url: row.get(4)?,
        original_url: row.get(5)?,
        thumbnail_url: row.get(6)?,
        thumbnail_local_path: row.get(7)?,
        local_path: row.get(8)?,
        width: row.get(9)?,
        height: row.get(10)?,
        aspect_ratio: row.get(11)?,
        file_size: row.get(12)?,
        mime_type: row.get(13)?,
        category: row.get(14)?,
        purity: row.get(15)?,
        hash: row.get(16)?,
        download_status: row.get(17)?,
        favorite: row.get(18)?,
        blacklisted: row.get(19)?,
        preset: row.get(20)?,
        created_at: row.get(21)?,
        synced_at: row.get(22)?,
        downloaded_at: row.get(23)?,
        last_used_at: row.get(24)?,
        tags: Vec::new(),
    })
}

/// Loads one wallpaper and its tags while the caller owns the connection lock.
fn load_wallpaper(
    connection: &Connection,
    wallpaper_id: i64,
) -> AppResult<Option<WallpaperRecord>> {
    let mut wallpaper = connection
        .query_row(
            "SELECT id, provider, remote_id, name, source_page_url, original_url, thumbnail_url,
                    thumbnail_local_path, local_path, width, height, aspect_ratio, file_size,
                    mime_type, category, purity, hash, download_status, favorite, blacklisted,
                    preset, created_at, synced_at, downloaded_at, last_used_at
             FROM wallpaper WHERE id = ?1",
            [wallpaper_id],
            map_wallpaper,
        )
        .optional()?;
    if let Some(record) = wallpaper.as_mut() {
        record.tags = load_tags(connection, record.id)?;
    }
    Ok(wallpaper)
}

/// Maps scheduler state and validates non-negative pool counts at the SQLite boundary.
fn map_schedule(row: &Row<'_>) -> rusqlite::Result<ScheduleRecord> {
    Ok(ScheduleRecord {
        system_monitor_id: row.get(0)?,
        enabled: row.get(1)?,
        paused: row.get(2)?,
        interval_seconds: row.get(3)?,
        fit_mode: row.get(4)?,
        last_change_at: row.get(5)?,
        next_change_at: row.get(6)?,
        last_error: row.get(7)?,
        wallpaper_count: row.get(8)?,
        selection_mode: row.get(9)?,
    })
}

/// Loads normalized tags separately so the main query remains one row per wallpaper.
fn load_tags(connection: &Connection, wallpaper_id: i64) -> AppResult<Vec<String>> {
    let mut statement = connection.prepare(
        "SELECT tag.name FROM tag
         JOIN wallpaper_tag ON wallpaper_tag.tag_id = tag.id
         WHERE wallpaper_tag.wallpaper_id = ?1 ORDER BY tag.name COLLATE NOCASE",
    )?;
    let rows = statement.query_map([wallpaper_id], |row| row.get(0))?;
    let mut tags = Vec::new();
    for row in rows {
        tags.push(row?);
    }
    Ok(tags)
}

/// Applies each migration once inside a transaction so partial schemas cannot persist.
fn apply_migrations(connection: &mut Connection) -> AppResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migration (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);",
    )?;
    for (version, sql) in MIGRATIONS {
        let applied: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migration WHERE version = ?1)",
            [version],
            |row| row.get(0),
        )?;
        if applied {
            continue;
        }
        let transaction = connection.transaction()?;
        transaction.execute_batch(sql)?;
        transaction.execute(
            "INSERT INTO schema_migration(version, applied_at)
             VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            [version],
        )?;
        transaction.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Database;
    use crate::models::{CatalogQuery, MonitorInfo, NewWallpaper};

    /// Creates realistic provider metadata shared by persistence tests.
    fn sample_wallpaper() -> NewWallpaper {
        NewWallpaper {
            provider: "wallhaven".into(),
            remote_id: "abc123".into(),
            name: "Mountain lake".into(),
            source_page_url: Some("https://wallhaven.cc/w/abc123".into()),
            original_url: Some("https://w.wallhaven.cc/full/ab/wallhaven-abc123.jpg".into()),
            thumbnail_url: Some("https://th.wallhaven.cc/small/ab/abc123.jpg".into()),
            thumbnail_local_path: Some("cache/abc123.jpg".into()),
            local_path: None,
            width: 3840,
            height: 2160,
            aspect_ratio: Some("16:9".into()),
            file_size: Some(2_000_000),
            mime_type: Some("image/jpeg".into()),
            category: "nature".into(),
            purity: "sfw".into(),
            hash: None,
            download_status: "remote".into(),
            preset: true,
            created_at: Some("2026-08-20T00:00:00Z".into()),
            synced_at: "2026-08-20T00:00:00Z".into(),
            tags: vec!["mountain".into(), "lake".into()],
        }
    }

    #[test]
    fn migrations_are_idempotent() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("test.db");
        let first = Database::open(&path)?;
        assert_eq!(first.schema_version()?, 4);
        drop(first);
        let second = Database::open(&path)?;
        assert_eq!(second.schema_version()?, 4);
        Ok(())
    }

    #[test]
    fn wallpaper_upsert_is_idempotent_and_keeps_tags() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        let wallpaper = sample_wallpaper();
        database.upsert_wallpapers(std::slice::from_ref(&wallpaper))?;
        database.upsert_wallpapers(std::slice::from_ref(&wallpaper))?;
        let page = database.list_wallpapers(1, 24, true)?;
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].remote_id, "abc123");
        assert_eq!(page.items[0].tags, vec!["lake", "mountain"]);
        Ok(())
    }

    #[test]
    fn catalog_search_filters_names_tags_categories_and_favorites()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        database.upsert_wallpapers(&[sample_wallpaper()])?;
        let wallpaper_id = database.list_wallpapers(1, 10, true)?.items[0].id;
        database.set_wallpaper_favorite(wallpaper_id, true)?;
        let page = database.search_wallpapers(&CatalogQuery {
            keyword: Some("lake".into()),
            category: Some("nature".into()),
            provider: Some("wallhaven".into()),
            favorite: Some(true),
            min_width: Some(3840),
            min_height: Some(2160),
            page: 1,
            page_size: 20,
            ..CatalogQuery::default()
        })?;
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].name, "Mountain lake");
        Ok(())
    }

    #[test]
    fn catalog_local_library_includes_downloaded_remote_originals()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        let remote_only = sample_wallpaper();
        let mut downloaded = sample_wallpaper();
        downloaded.remote_id = "downloaded123".into();
        downloaded.local_path = Some("wallpapers/original/downloaded123.jpg".into());
        downloaded.download_status = "downloaded".into();
        database.upsert_wallpapers(&[remote_only, downloaded])?;

        let page = database.search_wallpapers(&CatalogQuery {
            locally_available: true,
            page: 1,
            page_size: 20,
            ..CatalogQuery::default()
        })?;

        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].remote_id, "downloaded123");
        assert_eq!(page.items[0].provider, "wallhaven");
        Ok(())
    }

    #[test]
    fn settings_round_trip_json() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        let value = serde_json::json!({"intervalMinutes": 30});
        database.set_setting("scheduler", &value)?;
        assert_eq!(database.get_setting("scheduler")?, Some(value));
        Ok(())
    }

    #[test]
    fn configures_indexed_rotation_and_pause_resume() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        database.upsert_wallpapers(&[sample_wallpaper()])?;
        let wallpaper_id = database.list_wallpapers(1, 10, true)?.items[0].id;
        let schedule = database.configure_rotation(
            "DISPLAY-1",
            &[wallpaper_id],
            600,
            "fill",
            "round_robin",
        )?;
        assert_eq!(schedule.wallpaper_count, 1);
        assert_eq!(database.due_schedules()?.len(), 1);
        assert_eq!(
            database.next_rotation_wallpaper("DISPLAY-1")?.id,
            wallpaper_id
        );
        assert!(database.set_schedule_paused("DISPLAY-1", true)?.paused);
        assert!(database.due_schedules()?.is_empty());
        assert!(!database.set_schedule_paused("DISPLAY-1", false)?.paused);
        Ok(())
    }

    #[test]
    fn provider_refresh_preserves_disliked_tombstones() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        let wallpaper = sample_wallpaper();
        database.upsert_wallpapers(std::slice::from_ref(&wallpaper))?;
        let wallpaper_id = database.list_wallpapers(1, 10, false)?.items[0].id;
        database.set_wallpaper_blacklisted(wallpaper_id, true)?;

        // Synchronizing the same provider record updates metadata but must not resurrect it.
        database.upsert_wallpapers(std::slice::from_ref(&wallpaper))?;
        assert_eq!(database.list_wallpapers(1, 10, false)?.total, 0);
        let hidden = database.search_wallpapers(&CatalogQuery {
            include_blacklisted: true,
            page: 1,
            page_size: 10,
            ..CatalogQuery::default()
        })?;
        assert_eq!(hidden.total, 1);
        assert!(hidden.items[0].blacklisted);
        Ok(())
    }

    #[test]
    fn keeps_independent_selection_modes_per_monitor() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        database.configure_rotation("DISPLAY-1", &[], 600, "fill", "random")?;
        database.configure_rotation("DISPLAY-2", &[], 1800, "fit", "round_robin")?;

        let schedules = database.list_schedules()?;
        let display_one = schedules
            .iter()
            .find(|schedule| schedule.system_monitor_id == "DISPLAY-1")
            .ok_or("DISPLAY-1 schedule missing")?;
        let display_two = schedules
            .iter()
            .find(|schedule| schedule.system_monitor_id == "DISPLAY-2")
            .ok_or("DISPLAY-2 schedule missing")?;
        assert_eq!(display_one.selection_mode, "random");
        assert_eq!(display_one.interval_seconds, 600);
        assert_eq!(display_two.selection_mode, "round_robin");
        assert_eq!(display_two.interval_seconds, 1800);
        Ok(())
    }

    #[test]
    fn empty_pool_uses_recent_valid_files_and_excludes_blacklisted_or_deleted_items()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        database.upsert_monitors(&[MonitorInfo {
            system_monitor_id: "DISPLAY-1".into(),
            name: "Test Display".into(),
            width: 1920,
            height: 1080,
            position_x: 0,
            position_y: 0,
            primary: true,
        }])?;

        let valid_path = directory.path().join("valid.jpg");
        let blacklisted_path = directory.path().join("blacklisted.jpg");
        std::fs::write(&valid_path, b"available")?;
        std::fs::write(&blacklisted_path, b"available")?;
        let mut valid = sample_wallpaper();
        valid.remote_id = "valid".into();
        valid.local_path = Some(valid_path.display().to_string());
        valid.download_status = "downloaded".into();
        let mut blacklisted = valid.clone();
        blacklisted.remote_id = "blacklisted".into();
        blacklisted.local_path = Some(blacklisted_path.display().to_string());
        let mut deleted = valid.clone();
        deleted.remote_id = "deleted".into();
        deleted.local_path = Some(directory.path().join("deleted.jpg").display().to_string());
        database.upsert_wallpapers(&[valid, blacklisted, deleted])?;

        let records = database.list_wallpapers(1, 10, false)?.items;
        for wallpaper in &records {
            database.record_manual_history(wallpaper.id, "DISPLAY-1")?;
        }
        let blacklisted_id = records
            .iter()
            .find(|wallpaper| wallpaper.remote_id == "blacklisted")
            .ok_or("blacklisted fixture missing")?
            .id;
        database.set_wallpaper_blacklisted(blacklisted_id, true)?;
        let schedule = database.configure_rotation("DISPLAY-1", &[], 600, "fill", "round_robin")?;

        assert_eq!(schedule.wallpaper_count, 0);
        assert_eq!(
            database.next_rotation_wallpaper("DISPLAY-1")?.remote_id,
            "valid"
        );
        Ok(())
    }

    #[test]
    fn random_selected_pool_avoids_the_current_wallpaper_when_an_alternative_exists()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        database.upsert_monitors(&[MonitorInfo {
            system_monitor_id: "DISPLAY-1".into(),
            name: "Test Display".into(),
            width: 1920,
            height: 1080,
            position_x: 0,
            position_y: 0,
            primary: true,
        }])?;
        let first = sample_wallpaper();
        let mut second = sample_wallpaper();
        second.remote_id = "second".into();
        database.upsert_wallpapers(&[first, second])?;
        let records = database.list_wallpapers(1, 10, false)?.items;
        let current = records
            .iter()
            .find(|wallpaper| wallpaper.remote_id == "abc123")
            .ok_or("current fixture missing")?;
        let alternative = records
            .iter()
            .find(|wallpaper| wallpaper.remote_id == "second")
            .ok_or("alternative fixture missing")?;
        database.record_manual_history(current.id, "DISPLAY-1")?;
        database.configure_rotation(
            "DISPLAY-1",
            &[current.id, alternative.id],
            600,
            "fill",
            "random",
        )?;

        assert_eq!(
            database.next_rotation_wallpaper("DISPLAY-1")?.id,
            alternative.id
        );
        Ok(())
    }

    #[test]
    fn gallery_refresh_prunes_deleted_local_files_and_their_history()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        database.upsert_monitors(&[MonitorInfo {
            system_monitor_id: "DISPLAY-1".into(),
            name: "Test Display".into(),
            width: 1920,
            height: 1080,
            position_x: 0,
            position_y: 0,
            primary: true,
        }])?;
        let existing_path = directory.path().join("existing.jpg");
        std::fs::write(&existing_path, b"existing")?;
        let mut existing = sample_wallpaper();
        existing.provider = "local".into();
        existing.remote_id = "existing-local".into();
        existing.local_path = Some(existing_path.display().to_string());
        existing.download_status = "downloaded".into();
        let mut deleted = existing.clone();
        deleted.remote_id = "deleted-local".into();
        deleted.local_path = Some(directory.path().join("deleted.jpg").display().to_string());
        database.upsert_wallpapers(&[existing, deleted])?;
        let records = database.list_wallpapers(1, 10, false)?.items;
        let deleted_id = records
            .iter()
            .find(|wallpaper| wallpaper.remote_id == "deleted-local")
            .ok_or("deleted fixture missing")?
            .id;
        database.record_manual_history(deleted_id, "DISPLAY-1")?;

        assert_eq!(database.prune_missing_local_wallpapers()?, 1);
        assert!(database.get_wallpaper(deleted_id).is_err());
        assert!(existing_path.is_file());
        Ok(())
    }
}
