use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::Value};

use crate::{
    error::{AppError, AppResult},
    image_processing::ImageMetadata,
    models::{
        CatalogQuery, DuplicateFileCopy, DuplicateFileGroup, MonitorInfo, NewWallpaper,
        ScheduleRecord, WallpaperPage, WallpaperRecord,
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
    (
        5,
        include_str!("../../../migrations/0005_v2_gallery_file_state.sql"),
    ),
    (
        6,
        include_str!("../../../migrations/0006_v2_collections.sql"),
    ),
    (
        7,
        include_str!("../../../migrations/0007_v2_rotation_policy.sql"),
    ),
    (
        8,
        include_str!("../../../migrations/0008_v2_provider_center.sql"),
    ),
    (
        9,
        include_str!("../../../migrations/0009_v2_static_spanning.sql"),
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

    /// Returns enabled online providers while excluding LocalProvider from network work.
    pub fn enabled_online_providers(&self) -> AppResult<Vec<String>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT provider FROM provider_config
             WHERE enabled = 1 AND provider <> 'local' ORDER BY provider",
        )?;
        let rows = statement.query_map([], |row| row.get(0))?;
        let mut providers = Vec::new();
        for row in rows {
            providers.push(row?);
        }
        Ok(providers)
    }

    /// Lists independently persisted enablement and health state for provider diagnostics.
    pub fn list_provider_status(&self) -> AppResult<Vec<crate::models::ProviderStatus>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT config.provider, config.enabled, health.status, health.last_success_at,
                    health.last_error_at, health.last_error, health.response_time_ms
             FROM provider_config AS config
             JOIN provider_health AS health ON health.provider = config.provider
             ORDER BY config.provider",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(crate::models::ProviderStatus {
                provider: row.get(0)?,
                enabled: row.get(1)?,
                status: row.get(2)?,
                last_success_at: row.get(3)?,
                last_error_at: row.get(4)?,
                last_error: row.get(5)?,
                response_time_ms: row.get(6)?,
            })
        })?;
        let mut statuses = Vec::new();
        for row in rows {
            statuses.push(row?);
        }
        Ok(statuses)
    }

    /// Returns all attribution-bearing sources attached to one unified wallpaper entity.
    pub fn list_wallpaper_sources(
        &self,
        wallpaper_id: i64,
    ) -> AppResult<Vec<crate::models::WallpaperProviderSource>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT provider, remote_id, source_page_url, original_url, author,
                    license_name, license_url, width, height
             FROM wallpaper_provider_source WHERE wallpaper_id = ?1
             ORDER BY provider, remote_id",
        )?;
        let rows = statement.query_map([wallpaper_id], |row| {
            Ok(crate::models::WallpaperProviderSource {
                provider: row.get(0)?,
                remote_id: row.get(1)?,
                source_page_url: row.get(2)?,
                original_url: row.get(3)?,
                author: row.get(4)?,
                license_name: row.get(5)?,
                license_url: row.get(6)?,
                width: row.get(7)?,
                height: row.get(8)?,
            })
        })?;
        let mut sources = Vec::new();
        for row in rows {
            sources.push(row?);
        }
        Ok(sources)
    }

    /// Toggles one built-in provider without introducing a global default provider value.
    pub fn set_provider_enabled(&self, provider: &str, enabled: bool) -> AppResult<()> {
        let connection = self.lock()?;
        let changed = connection.execute(
            "UPDATE provider_config SET enabled = ?2,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE provider = ?1",
            params![provider, enabled],
        )?;
        if changed != 1 {
            return Err(AppError::Provider(format!("unknown provider: {provider}")));
        }
        Ok(())
    }

    /// Records one provider result without allowing its failure to roll back other sources.
    pub fn record_provider_health(
        &self,
        provider: &str,
        elapsed_ms: u128,
        error: Option<&str>,
    ) -> AppResult<()> {
        let elapsed_ms = i64::try_from(elapsed_ms)
            .map_err(|_| AppError::Database("provider response time is out of range".into()))?;
        let connection = self.lock()?;
        connection.execute(
            "UPDATE provider_health SET
                status = CASE WHEN ?3 IS NULL THEN 'healthy' ELSE 'degraded' END,
                last_success_at = CASE WHEN ?3 IS NULL THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') ELSE last_success_at END,
                last_error_at = CASE WHEN ?3 IS NOT NULL THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') ELSE last_error_at END,
                last_error = ?3,
                response_time_ms = ?2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE provider = ?1",
            params![provider, elapsed_ms, error],
        )?;
        Ok(())
    }

    /// Persists the exact geometry and rollback paths only after every span slice was applied.
    pub fn save_spanning_assignment(
        &self,
        wallpaper_id: i64,
        layout: &crate::span::MonitorLayout,
        fit_mode: &str,
        previous: &[crate::span::PreviousWallpaper],
    ) -> AppResult<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let layout_json = serde_json::to_string(layout)?;
        let previous_json = serde_json::to_string(previous)?;
        transaction.execute(
            "INSERT INTO monitor_layout_snapshot(layout_hash, layout_json, created_at)
             VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(layout_hash) DO UPDATE SET layout_json = excluded.layout_json",
            params![layout.layout_hash, layout_json],
        )?;
        let snapshot_id: i64 = transaction.query_row(
            "SELECT id FROM monitor_layout_snapshot WHERE layout_hash = ?1",
            [&layout.layout_hash],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO spanning_assignment(
                id, wallpaper_id, layout_snapshot_id, fit_mode, previous_paths_json,
                active, last_error, updated_at
             ) VALUES (1, ?1, ?2, ?3, ?4, 1, NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(id) DO UPDATE SET
                wallpaper_id = excluded.wallpaper_id,
                layout_snapshot_id = excluded.layout_snapshot_id,
                fit_mode = excluded.fit_mode,
                previous_paths_json = excluded.previous_paths_json,
                active = 1,
                last_error = NULL,
                updated_at = excluded.updated_at",
            params![wallpaper_id, snapshot_id, fit_mode, previous_json],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Returns rollback paths from the active span instead of capturing already-sliced images.
    pub fn active_spanning_previous(&self) -> AppResult<Vec<crate::span::PreviousWallpaper>> {
        let connection = self.lock()?;
        let serialized = connection
            .query_row(
                "SELECT previous_paths_json FROM spanning_assignment WHERE id = 1 AND active = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        serialized
            .map(|value| serde_json::from_str(&value).map_err(Into::into))
            .unwrap_or_else(|| Ok(Vec::new()))
    }

    /// Marks spanning inactive after the platform adapter restores all independent assignments.
    pub fn deactivate_spanning(&self, last_error: Option<&str>) -> AppResult<()> {
        let connection = self.lock()?;
        connection.execute(
            "UPDATE spanning_assignment SET active = 0, last_error = ?1,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = 1",
            [last_error],
        )?;
        Ok(())
    }

    /// Retains the concrete display failure for diagnostics after best-effort rollback.
    pub fn save_spanning_failure(
        &self,
        wallpaper_id: i64,
        fit_mode: &str,
        previous: &[crate::span::PreviousWallpaper],
        error: &str,
    ) -> AppResult<()> {
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO spanning_assignment(
                id, wallpaper_id, layout_snapshot_id, fit_mode, previous_paths_json,
                active, last_error, updated_at
             ) VALUES (1, ?1, NULL, ?2, ?3, 0, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(id) DO UPDATE SET
                wallpaper_id = excluded.wallpaper_id,
                fit_mode = excluded.fit_mode,
                previous_paths_json = excluded.previous_paths_json,
                active = 0,
                last_error = excluded.last_error,
                updated_at = excluded.updated_at",
            params![
                wallpaper_id,
                fit_mode,
                serde_json::to_string(previous)?,
                error
            ],
        )?;
        Ok(())
    }

    /// Inserts or refreshes provider metadata while preserving user-owned catalog state.
    pub fn upsert_wallpapers(&self, wallpapers: &[NewWallpaper]) -> AppResult<usize> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let mut upserted = 0_usize;
        for wallpaper in wallpapers {
            let canonical_original_url = wallpaper
                .original_url
                .as_deref()
                .and_then(normalize_original_url);
            let excluded: bool = transaction.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM wallpaper_exclusion
                    WHERE (provider = ?1 AND remote_id = ?2)
                       OR (?3 IS NOT NULL AND normalized_path = ?3)
                       OR (?4 IS NOT NULL AND content_hash = ?4)
                 )",
                params![
                    wallpaper.provider,
                    wallpaper.remote_id,
                    wallpaper.local_path,
                    wallpaper.hash,
                ],
                |row| row.get(0),
            )?;
            if excluded {
                continue;
            }
            let existing_id = transaction
                .query_row(
                    "SELECT wallpaper_id FROM wallpaper_provider_source
                     WHERE provider = ?1 AND remote_id = ?2
                     UNION ALL
                     SELECT wallpaper_id FROM wallpaper_content_identity
                     WHERE ?3 IS NOT NULL AND canonical_original_url = ?3
                     UNION ALL
                     SELECT wallpaper_id FROM wallpaper_content_identity
                     WHERE ?4 IS NOT NULL AND sha256 = ?4
                     UNION ALL
                     SELECT wallpaper_id FROM wallpaper_content_identity
                     WHERE ?5 IS NOT NULL AND perceptual_hash = ?5
                       AND width > 0 AND height > 0 AND ?6 > 0 AND ?7 > 0
                       AND ABS((width * ?7) - (?6 * height)) * 100
                           <= MAX(width * ?7, ?6 * height) * 2
                     LIMIT 1",
                    params![
                        wallpaper.provider,
                        wallpaper.remote_id,
                        canonical_original_url,
                        wallpaper.hash,
                        wallpaper.perceptual_hash,
                        wallpaper.width,
                        wallpaper.height,
                    ],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;

            let wallpaper_id = if let Some(wallpaper_id) = existing_id {
                // A higher-resolution source may improve the unified card without replacing user state.
                transaction.execute(
                    "UPDATE wallpaper SET
                        name = CASE WHEN (?2 * ?3) > (width * height) THEN ?4 ELSE name END,
                        source_page_url = CASE WHEN (?2 * ?3) > (width * height) THEN ?5 ELSE source_page_url END,
                        original_url = CASE WHEN (?2 * ?3) > (width * height) THEN ?6 ELSE original_url END,
                        thumbnail_url = CASE WHEN (?2 * ?3) > (width * height) THEN ?7 ELSE thumbnail_url END,
                        width = CASE WHEN (?2 * ?3) > (width * height) THEN ?2 ELSE width END,
                        height = CASE WHEN (?2 * ?3) > (width * height) THEN ?3 ELSE height END,
                        aspect_ratio = CASE WHEN (?2 * ?3) > (width * height) THEN ?8 ELSE aspect_ratio END,
                        file_size = COALESCE(file_size, ?9),
                        mime_type = COALESCE(mime_type, ?10),
                        synced_at = ?11,
                        preset = MAX(preset, ?12)
                     WHERE id = ?1",
                    params![
                        wallpaper_id,
                        wallpaper.width,
                        wallpaper.height,
                        wallpaper.name,
                        wallpaper.source_page_url,
                        wallpaper.original_url,
                        wallpaper.thumbnail_url,
                        wallpaper.aspect_ratio,
                        wallpaper.file_size,
                        wallpaper.mime_type,
                        wallpaper.synced_at,
                        wallpaper.preset,
                    ],
                )?;
                wallpaper_id
            } else {
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
                    local_path = CASE WHEN excluded.provider = 'local' THEN excluded.local_path ELSE wallpaper.local_path END,
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
                transaction.query_row(
                    "SELECT id FROM wallpaper WHERE provider = ?1 AND remote_id = ?2",
                    params![wallpaper.provider, wallpaper.remote_id],
                    |row| row.get(0),
                )?
            };

            transaction.execute(
                "INSERT INTO wallpaper_provider_source(
                    wallpaper_id, provider, remote_id, source_page_url, original_url,
                    author, license_name, license_url, width, height, file_size, mime_type, last_seen_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                           strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                 ON CONFLICT(provider, remote_id) DO UPDATE SET
                    wallpaper_id = excluded.wallpaper_id,
                    source_page_url = excluded.source_page_url,
                    original_url = excluded.original_url,
                    author = excluded.author,
                    license_name = excluded.license_name,
                    license_url = excluded.license_url,
                    width = excluded.width,
                    height = excluded.height,
                    file_size = excluded.file_size,
                    mime_type = excluded.mime_type,
                    last_seen_at = excluded.last_seen_at",
                params![
                    wallpaper_id,
                    wallpaper.provider,
                    wallpaper.remote_id,
                    wallpaper.source_page_url,
                    wallpaper.original_url,
                    wallpaper.author,
                    wallpaper.license_name,
                    wallpaper.license_url,
                    wallpaper.width,
                    wallpaper.height,
                    wallpaper.file_size,
                    wallpaper.mime_type,
                ],
            )?;
            transaction.execute(
                "INSERT INTO wallpaper_content_identity(
                    wallpaper_id, canonical_original_url, sha256, perceptual_hash,
                    width, height, confidence, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6,
                           CASE WHEN ?3 IS NOT NULL THEN 'hash' WHEN ?2 IS NOT NULL THEN 'url'
                                WHEN ?4 IS NOT NULL THEN 'perceptual' ELSE 'provider' END,
                           strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                 ON CONFLICT(wallpaper_id) DO UPDATE SET
                    canonical_original_url = COALESCE(wallpaper_content_identity.canonical_original_url, excluded.canonical_original_url),
                    sha256 = COALESCE(excluded.sha256, wallpaper_content_identity.sha256),
                    perceptual_hash = COALESCE(excluded.perceptual_hash, wallpaper_content_identity.perceptual_hash),
                    width = MAX(wallpaper_content_identity.width, excluded.width),
                    height = MAX(wallpaper_content_identity.height, excluded.height),
                    confidence = CASE WHEN excluded.sha256 IS NOT NULL THEN 'hash'
                                      WHEN excluded.perceptual_hash IS NOT NULL AND wallpaper_content_identity.confidence = 'provider'
                                      THEN 'perceptual' ELSE wallpaper_content_identity.confidence END,
                    updated_at = excluded.updated_at",
                params![
                    wallpaper_id,
                    canonical_original_url,
                    wallpaper.hash,
                    wallpaper.perceptual_hash,
                    wallpaper.width,
                    wallpaper.height,
                ],
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
            if let Some(path) = wallpaper.local_path.as_deref() {
                let storage_kind = if wallpaper.provider == "local" {
                    "user_source"
                } else {
                    "managed_download"
                };
                transaction.execute(
                    "INSERT INTO wallpaper_file_state(
                        wallpaper_id, path, storage_kind, availability, file_size,
                        content_hash, last_verified_at, missing_since
                     ) VALUES (?1, ?2, ?3, 'available', ?4, ?5,
                        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), NULL)
                     ON CONFLICT(path) DO UPDATE SET
                        wallpaper_id = excluded.wallpaper_id,
                        storage_kind = excluded.storage_kind,
                        availability = 'available',
                        file_size = excluded.file_size,
                        content_hash = excluded.content_hash,
                        last_verified_at = excluded.last_verified_at,
                        missing_since = NULL",
                    params![
                        wallpaper_id,
                        path,
                        storage_kind,
                        wallpaper.file_size,
                        wallpaper.hash,
                    ],
                )?;
            }
            upserted += 1;
        }
        transaction.commit()?;
        Ok(upserted)
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
                    preset, created_at, synced_at, downloaded_at, last_used_at,
                    COALESCE((SELECT state.availability FROM wallpaper_file_state AS state
                              WHERE state.wallpaper_id = wallpaper.id
                              ORDER BY CASE state.availability WHEN 'available' THEN 0 WHEN 'temporarily_unavailable' THEN 1 ELSE 2 END, state.id
                              LIMIT 1), 'remote') AS file_availability,
                    COALESCE((SELECT state.storage_kind FROM wallpaper_file_state AS state
                              WHERE state.wallpaper_id = wallpaper.id
                              ORDER BY CASE state.availability WHEN 'available' THEN 0 WHEN 'temporarily_unavailable' THEN 1 ELSE 2 END, state.id
                              LIMIT 1), 'remote_metadata') AS storage_kind,
                    (SELECT COUNT(*) FROM wallpaper_file_state AS state WHERE state.wallpaper_id = wallpaper.id) AS file_copy_count
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
            predicates.push("EXISTS (SELECT 1 FROM wallpaper_file_state AS state WHERE state.wallpaper_id = wallpaper.id AND state.availability = 'available')".to_owned());
        }
        if query.file_backed {
            predicates.push("EXISTS (SELECT 1 FROM wallpaper_file_state AS state WHERE state.wallpaper_id = wallpaper.id)".to_owned());
        }
        if let Some(download_status) = query.download_status.as_deref() {
            let slot = values.len() + 1;
            predicates.push(format!("wallpaper.download_status = ?{slot}"));
            values.push(Value::Text(download_status.to_owned()));
        }
        if let Some(availability) = query.file_availability.as_deref() {
            let slot = values.len() + 1;
            if availability == "remote" {
                predicates.push("NOT EXISTS (SELECT 1 FROM wallpaper_file_state AS state WHERE state.wallpaper_id = wallpaper.id)".to_owned());
            } else {
                predicates.push(format!("EXISTS (SELECT 1 FROM wallpaper_file_state AS state WHERE state.wallpaper_id = wallpaper.id AND state.availability = ?{slot})"));
                values.push(Value::Text(availability.to_owned()));
            }
        }
        if let Some(storage_kind) = query.storage_kind.as_deref() {
            let slot = values.len() + 1;
            if storage_kind == "remote_metadata" {
                predicates.push("NOT EXISTS (SELECT 1 FROM wallpaper_file_state AS state WHERE state.wallpaper_id = wallpaper.id)".to_owned());
            } else {
                predicates.push(format!("EXISTS (SELECT 1 FROM wallpaper_file_state AS state WHERE state.wallpaper_id = wallpaper.id AND state.storage_kind = ?{slot})"));
                values.push(Value::Text(storage_kind.to_owned()));
            }
        }
        if let Some(collection_id) = query.collection_id.filter(|id| *id > 0) {
            let slot = values.len() + 1;
            predicates.push(format!("EXISTS (SELECT 1 FROM collection_wallpaper AS member WHERE member.wallpaper_id = wallpaper.id AND member.collection_id = ?{slot})"));
            values.push(Value::Integer(collection_id));
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
        if let Some(max_width) = query.max_width.filter(|value| *value > 0) {
            let slot = values.len() + 1;
            predicates.push(format!("wallpaper.width <= ?{slot}"));
            values.push(Value::Integer(i64::from(max_width)));
        }
        if let Some(max_height) = query.max_height.filter(|value| *value > 0) {
            let slot = values.len() + 1;
            predicates.push(format!("wallpaper.height <= ?{slot}"));
            values.push(Value::Integer(i64::from(max_height)));
        }
        if let Some(aspect_ratio) = query.aspect_ratio.as_deref() {
            let slot = values.len() + 1;
            predicates.push(format!("wallpaper.aspect_ratio = ?{slot}"));
            values.push(Value::Text(aspect_ratio.to_owned()));
        }
        if let Some(mime_type) = query.mime_type.as_deref() {
            let slot = values.len() + 1;
            predicates.push(format!("wallpaper.mime_type = ?{slot}"));
            values.push(Value::Text(mime_type.to_owned()));
        }
        for tag in query
            .tags
            .iter()
            .map(|tag| tag.trim())
            .filter(|tag| !tag.is_empty())
        {
            let slot = values.len() + 1;
            predicates.push(format!("EXISTS (SELECT 1 FROM wallpaper_tag AS wt JOIN tag AS filter_tag ON filter_tag.id = wt.tag_id WHERE wt.wallpaper_id = wallpaper.id AND filter_tag.name = ?{slot} COLLATE NOCASE)"));
            values.push(Value::Text(tag.to_owned()));
        }

        let where_clause = predicates.join(" AND ");
        let order_clause = match query.sort.as_deref() {
            Some("random") => "RANDOM()",
            Some("name") => "wallpaper.name COLLATE NOCASE ASC, wallpaper.id ASC",
            Some("recently_used") => "wallpaper.last_used_at DESC, wallpaper.id DESC",
            Some("file_size") => "wallpaper.file_size DESC, wallpaper.id DESC",
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
                    preset, created_at, synced_at, downloaded_at, last_used_at,
                    COALESCE((SELECT state.availability FROM wallpaper_file_state AS state
                              WHERE state.wallpaper_id = wallpaper.id
                              ORDER BY CASE state.availability WHEN 'available' THEN 0 WHEN 'temporarily_unavailable' THEN 1 ELSE 2 END, state.id
                              LIMIT 1), 'remote') AS file_availability,
                    COALESCE((SELECT state.storage_kind FROM wallpaper_file_state AS state
                              WHERE state.wallpaper_id = wallpaper.id
                              ORDER BY CASE state.availability WHEN 'available' THEN 0 WHEN 'temporarily_unavailable' THEN 1 ELSE 2 END, state.id
                              LIMIT 1), 'remote_metadata') AS storage_kind,
                    (SELECT COUNT(*) FROM wallpaper_file_state AS state WHERE state.wallpaper_id = wallpaper.id) AS file_copy_count
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

    /// Lists content hashes with multiple indexed file locations for an explicit duplicate view.
    pub fn list_duplicate_file_groups(&self) -> AppResult<Vec<DuplicateFileGroup>> {
        let connection = self.lock()?;
        let mut hashes = connection.prepare(
            "SELECT content_hash FROM wallpaper_file_state
             WHERE content_hash IS NOT NULL
             GROUP BY content_hash HAVING COUNT(*) > 1
             ORDER BY COUNT(*) DESC, content_hash",
        )?;
        let hash_rows = hashes.query_map([], |row| row.get::<_, String>(0))?;
        let mut groups = Vec::new();
        for hash in hash_rows {
            let hash = hash?;
            let mut copies_statement = connection.prepare(
                "SELECT wallpaper_id, path, storage_kind, availability, file_size
                 FROM wallpaper_file_state WHERE content_hash = ?1
                 ORDER BY availability, storage_kind, path",
            )?;
            let copy_rows = copies_statement.query_map([&hash], |row| {
                Ok(DuplicateFileCopy {
                    wallpaper_id: row.get(0)?,
                    path: row.get(1)?,
                    storage_kind: row.get(2)?,
                    availability: row.get(3)?,
                    file_size: row.get(4)?,
                })
            })?;
            let mut copies = Vec::new();
            for copy in copy_rows {
                copies.push(copy?);
            }
            groups.push(DuplicateFileGroup {
                content_hash: hash,
                copies,
            });
        }
        Ok(groups)
    }

    /// Returns collection summaries in stable user-defined order.
    pub fn list_collections(&self) -> AppResult<Vec<crate::models::CollectionRecord>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT collection.id, collection.name, collection.description,
                    collection.cover_wallpaper_id, collection.position,
                    COUNT(collection_wallpaper.wallpaper_id),
                    EXISTS(SELECT 1 FROM smart_collection_rule WHERE collection_id = collection.id)
             FROM collection
             LEFT JOIN collection_wallpaper ON collection_wallpaper.collection_id = collection.id
             GROUP BY collection.id
             ORDER BY collection.position, collection.name COLLATE NOCASE, collection.id",
        )?;
        let rows = statement.query_map([], |row| {
            let count: i64 = row.get(5)?;
            Ok(crate::models::CollectionRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                cover_wallpaper_id: row.get(3)?,
                position: row.get(4)?,
                wallpaper_count: u64::try_from(count)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(5, count))?,
                smart: row.get(6)?,
            })
        })?;
        let mut collections = Vec::new();
        for row in rows {
            collections.push(row?);
        }
        Ok(collections)
    }

    /// Inserts one manual collection and returns its persisted summary.
    pub fn create_collection(
        &self,
        name: &str,
        description: &str,
    ) -> AppResult<crate::models::CollectionRecord> {
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO collection(name, description, position, created_at, updated_at)
             VALUES (?1, ?2, COALESCE((SELECT MAX(position) + 1 FROM collection), 0),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![name, description],
        )?;
        let id = connection.last_insert_rowid();
        drop(connection);
        self.get_collection(id)
    }

    /// Updates collection presentation fields while validating the optional cover reference.
    pub fn update_collection(
        &self,
        collection_id: i64,
        name: &str,
        description: &str,
        cover_wallpaper_id: Option<i64>,
        position: i64,
    ) -> AppResult<crate::models::CollectionRecord> {
        let connection = self.lock()?;
        let changed = connection.execute(
            "UPDATE collection SET name = ?2, description = ?3, cover_wallpaper_id = ?4,
                    position = ?5, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1",
            params![
                collection_id,
                name,
                description,
                cover_wallpaper_id,
                position
            ],
        )?;
        if changed != 1 {
            return Err(AppError::Wallpaper("collection does not exist".into()));
        }
        drop(connection);
        self.get_collection(collection_id)
    }

    /// Deletes only a collection; cascading membership rows do not affect wallpaper records.
    pub fn delete_collection(&self, collection_id: i64) -> AppResult<()> {
        let connection = self.lock()?;
        let changed =
            connection.execute("DELETE FROM collection WHERE id = ?1", [collection_id])?;
        if changed != 1 {
            return Err(AppError::Wallpaper("collection does not exist".into()));
        }
        Ok(())
    }

    /// Adds existing non-blacklisted wallpapers and reports newly created relationships.
    pub fn add_collection_wallpapers(
        &self,
        collection_id: i64,
        wallpaper_ids: &[i64],
    ) -> AppResult<usize> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM collection WHERE id = ?1)",
            [collection_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(AppError::Wallpaper("collection does not exist".into()));
        }
        let mut added = 0_usize;
        for wallpaper_id in wallpaper_ids.iter().copied().take(10_000) {
            added += transaction.execute(
                "INSERT INTO collection_wallpaper(collection_id, wallpaper_id, position, added_at)
                 SELECT ?1, id,
                        COALESCE((SELECT MAX(position) + 1 FROM collection_wallpaper WHERE collection_id = ?1), 0),
                        strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 FROM wallpaper WHERE id = ?2 AND blacklisted = 0
                 ON CONFLICT(collection_id, wallpaper_id) DO NOTHING",
                params![collection_id, wallpaper_id],
            )?;
        }
        transaction.execute(
            "UPDATE collection SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
            [collection_id],
        )?;
        transaction.commit()?;
        Ok(added)
    }

    /// Removes only requested collection membership links in one transaction.
    pub fn remove_collection_wallpapers(
        &self,
        collection_id: i64,
        wallpaper_ids: &[i64],
    ) -> AppResult<usize> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let mut removed = 0_usize;
        for wallpaper_id in wallpaper_ids.iter().copied().take(10_000) {
            removed += transaction.execute(
                "DELETE FROM collection_wallpaper WHERE collection_id = ?1 AND wallpaper_id = ?2",
                params![collection_id, wallpaper_id],
            )?;
        }
        transaction.execute(
            "UPDATE collection SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
            [collection_id],
        )?;
        transaction.commit()?;
        Ok(removed)
    }

    /// Stores a serialized versioned rule after the service has validated its schema version.
    pub fn set_smart_collection_rule(
        &self,
        collection_id: i64,
        rule: &crate::models::SmartCollectionRule,
    ) -> AppResult<()> {
        let rule_json = serde_json::to_string(rule)?;
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO smart_collection_rule(collection_id, version, rule_json, updated_at)
             VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(collection_id) DO UPDATE SET
                version = excluded.version,
                rule_json = excluded.rule_json,
                updated_at = excluded.updated_at",
            params![collection_id, rule.version, rule_json],
        )?;
        Ok(())
    }

    /// Evaluates a smart rule through the shared catalog query rather than interpolated SQL.
    pub fn preview_smart_collection(
        &self,
        rule: &crate::models::SmartCollectionRule,
        page: u32,
        page_size: u32,
    ) -> AppResult<WallpaperPage> {
        self.search_wallpapers(&CatalogQuery {
            provider: rule.provider.clone(),
            category: rule.category.clone(),
            favorite: rule.favorite,
            file_availability: rule.file_availability.clone(),
            min_width: rule.min_width,
            min_height: rule.min_height,
            aspect_ratio: rule.aspect_ratio.clone(),
            tags: rule.tags.clone(),
            page,
            page_size,
            ..CatalogQuery::default()
        })
    }

    /// Resolves manual membership or the persisted smart rule through one paginated API.
    pub fn query_collection_wallpapers(
        &self,
        collection_id: i64,
        page: u32,
        page_size: u32,
    ) -> AppResult<WallpaperPage> {
        let connection = self.lock()?;
        let rule_json = connection
            .query_row(
                "SELECT rule_json FROM smart_collection_rule WHERE collection_id = ?1",
                [collection_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        drop(connection);
        if let Some(rule_json) = rule_json {
            let rule: crate::models::SmartCollectionRule = serde_json::from_str(&rule_json)?;
            return self.preview_smart_collection(&rule, page, page_size);
        }
        self.search_wallpapers(&CatalogQuery {
            collection_id: Some(collection_id),
            page,
            page_size,
            ..CatalogQuery::default()
        })
    }

    /// Loads one collection summary after a mutation.
    fn get_collection(&self, collection_id: i64) -> AppResult<crate::models::CollectionRecord> {
        self.list_collections()?
            .into_iter()
            .find(|collection| collection.id == collection_id)
            .ok_or_else(|| AppError::Wallpaper("collection does not exist".into()))
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

    /// Resolves the retained catalog identity after original-content hash deduplication.
    pub fn wallpaper_by_hash(&self, sha256: &str) -> AppResult<WallpaperRecord> {
        let connection = self.lock()?;
        let wallpaper_id = connection
            .query_row(
                "SELECT id FROM wallpaper WHERE hash = ?1 LIMIT 1",
                [sha256],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| {
                AppError::Wallpaper("downloaded wallpaper identity is missing".into())
            })?;
        drop(connection);
        self.get_wallpaper(wallpaper_id)
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
        let mut connection = connection;
        let transaction = connection.transaction()?;
        let duplicate_id = transaction
            .query_row(
                "SELECT wallpaper_id FROM wallpaper_content_identity
                 WHERE wallpaper_id <> ?2
                   AND (sha256 = ?1 OR (
                        perceptual_hash = ?3 AND width > 0 AND height > 0
                        AND ABS((width * ?5) - (?4 * height)) * 100
                            <= MAX(width * ?5, ?4 * height) * 2
                   ))
                 ORDER BY CASE WHEN sha256 = ?1 THEN 0 ELSE 1 END
                 LIMIT 1",
                params![
                    metadata.sha256,
                    wallpaper_id,
                    metadata.perceptual_hash,
                    metadata.width,
                    metadata.height,
                ],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let retained_id = duplicate_id.unwrap_or(wallpaper_id);

        if retained_id != wallpaper_id {
            // Move every user-owned relationship before deleting the redundant metadata card.
            transaction.execute(
                "UPDATE wallpaper SET
                    favorite = MAX(favorite, (SELECT favorite FROM wallpaper WHERE id = ?2)),
                    blacklisted = MAX(blacklisted, (SELECT blacklisted FROM wallpaper WHERE id = ?2)),
                    preset = MAX(preset, (SELECT preset FROM wallpaper WHERE id = ?2))
                 WHERE id = ?1",
                params![retained_id, wallpaper_id],
            )?;
            transaction.execute(
                "UPDATE wallpaper_provider_source SET wallpaper_id = ?1 WHERE wallpaper_id = ?2",
                params![retained_id, wallpaper_id],
            )?;
            transaction.execute(
                "INSERT OR IGNORE INTO wallpaper_tag(wallpaper_id, tag_id)
                 SELECT ?1, tag_id FROM wallpaper_tag WHERE wallpaper_id = ?2",
                params![retained_id, wallpaper_id],
            )?;
            transaction.execute(
                "INSERT OR IGNORE INTO collection_wallpaper(collection_id, wallpaper_id, position, added_at)
                 SELECT collection_id, ?1, position, added_at FROM collection_wallpaper WHERE wallpaper_id = ?2",
                params![retained_id, wallpaper_id],
            )?;
            transaction.execute(
                "INSERT OR IGNORE INTO rotation_wallpaper(system_monitor_id, wallpaper_id, selected_at)
                 SELECT system_monitor_id, ?1, selected_at FROM rotation_wallpaper WHERE wallpaper_id = ?2",
                params![retained_id, wallpaper_id],
            )?;
            transaction.execute(
                "INSERT OR IGNORE INTO rotation_queue(system_monitor_id, generation, wallpaper_id, position, consumed_at)
                 SELECT system_monitor_id, generation, ?1, position, consumed_at FROM rotation_queue WHERE wallpaper_id = ?2",
                params![retained_id, wallpaper_id],
            )?;
            transaction.execute(
                "UPDATE wallpaper_history SET wallpaper_id = ?1 WHERE wallpaper_id = ?2",
                params![retained_id, wallpaper_id],
            )?;
            transaction.execute(
                "UPDATE wallpaper_file_state SET wallpaper_id = ?1 WHERE wallpaper_id = ?2",
                params![retained_id, wallpaper_id],
            )?;
            transaction.execute(
                "UPDATE collection SET cover_wallpaper_id = ?1 WHERE cover_wallpaper_id = ?2",
                params![retained_id, wallpaper_id],
            )?;
            transaction.execute("DELETE FROM wallpaper WHERE id = ?1", [wallpaper_id])?;
        }
        let updated = transaction.execute(
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
                retained_id,
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
        transaction.execute(
            "INSERT INTO wallpaper_file_state(
                wallpaper_id, path, storage_kind, availability, file_size,
                content_hash, last_verified_at, missing_since
             ) VALUES (?1, ?2, 'managed_download', 'available', ?3, ?4,
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), NULL)
             ON CONFLICT(path) DO UPDATE SET
                wallpaper_id = excluded.wallpaper_id,
                availability = 'available',
                file_size = excluded.file_size,
                content_hash = excluded.content_hash,
                last_verified_at = excluded.last_verified_at,
                missing_since = NULL",
            params![
                retained_id,
                local_path.display().to_string(),
                file_size,
                metadata.sha256,
            ],
        )?;
        transaction.execute(
            "UPDATE wallpaper_content_identity SET
                sha256 = ?2, perceptual_hash = ?3, width = ?4, height = ?5, confidence = 'hash',
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE wallpaper_id = ?1",
            params![
                retained_id,
                metadata.sha256,
                metadata.perceptual_hash,
                metadata.width,
                metadata.height
            ],
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_wallpaper(retained_id)
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
                "INSERT OR IGNORE INTO wallpaper_exclusion(
                    provider, remote_id, normalized_path, content_hash, reason, created_at
                 ) SELECT provider, remote_id, local_path, hash, 'blacklisted',
                          strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                   FROM wallpaper WHERE id = ?1",
                [wallpaper_id],
            )?;
            transaction.execute(
                "DELETE FROM rotation_wallpaper WHERE wallpaper_id = ?1",
                [wallpaper_id],
            )?;
        } else {
            transaction.execute(
                "DELETE FROM wallpaper_exclusion
                 WHERE reason = 'blacklisted'
                   AND EXISTS (
                       SELECT 1 FROM wallpaper
                       WHERE wallpaper.id = ?1
                         AND (wallpaper_exclusion.provider = wallpaper.provider
                              AND wallpaper_exclusion.remote_id = wallpaper.remote_id)
                   )",
                [wallpaper_id],
            )?;
        }
        transaction.commit()?;
        drop(connection);
        self.get_wallpaper(wallpaper_id)
    }

    /// Clears one remote original reference after the command layer has validated ownership.
    pub fn clear_wallpaper_download(&self, wallpaper_id: i64) -> AppResult<WallpaperRecord> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let local_path = transaction
            .query_row(
                "SELECT local_path FROM wallpaper WHERE id = ?1 AND provider <> 'local'",
                [wallpaper_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        let changed = transaction.execute(
            "UPDATE wallpaper SET local_path = NULL, hash = NULL, download_status = 'remote', downloaded_at = NULL WHERE id = ?1 AND provider <> 'local'",
            [wallpaper_id],
        )?;
        if changed != 1 {
            return Err(AppError::Wallpaper(
                "only downloaded remote wallpaper cache can be removed".into(),
            ));
        }
        if let Some(path) = local_path {
            transaction.execute(
                "DELETE FROM wallpaper_file_state
                 WHERE wallpaper_id = ?1 AND path = ?2 AND storage_kind = 'managed_download'",
                params![wallpaper_id, path],
            )?;
        }
        transaction.commit()?;
        drop(connection);
        self.get_wallpaper(wallpaper_id)
    }

    /// Removes a LocalProvider index and dependent history without touching the user-owned file.
    pub fn remove_local_wallpaper_index(&self, wallpaper_id: i64) -> AppResult<()> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let (provider, remote_id, local_path, hash) = transaction
            .query_row(
                "SELECT provider, remote_id, local_path, hash FROM wallpaper WHERE id = ?1",
                [wallpaper_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| AppError::Wallpaper("wallpaper does not exist".into()))?;
        if provider != "local" {
            return Err(AppError::Wallpaper(
                "only LocalProvider indexes can be removed by this operation".into(),
            ));
        }
        transaction.execute(
            "INSERT OR IGNORE INTO wallpaper_exclusion(
                provider, remote_id, normalized_path, content_hash, reason, created_at
             ) VALUES (?1, ?2, ?3, ?4, 'index_removed', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![provider, remote_id, local_path, hash],
        )?;
        transaction.execute(
            "DELETE FROM wallpaper_history WHERE wallpaper_id = ?1",
            [wallpaper_id],
        )?;
        transaction.execute("DELETE FROM wallpaper WHERE id = ?1", [wallpaper_id])?;
        transaction.commit()?;
        Ok(())
    }

    /// Reconciles missing files while distinguishing an offline root from confirmed deletion.
    pub fn reconcile_local_file_states(&self, tracked_roots: &[String]) -> AppResult<usize> {
        let mut connection = self.lock()?;
        let changes = {
            let mut statement = connection.prepare(
                "SELECT id, path FROM wallpaper_file_state
                 WHERE storage_kind = 'user_source'",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            let roots: Vec<_> = tracked_roots.iter().map(Path::new).collect();
            let mut states = Vec::new();
            for row in rows {
                let (file_state_id, path) = row?;
                let source = Path::new(&path);
                let owning_root = roots.iter().find(|root| source.starts_with(root));
                if source.is_file() {
                    states.push((file_state_id, "available"));
                } else if owning_root.is_some_and(|root| root.is_dir()) {
                    states.push((file_state_id, "missing"));
                } else if owning_root.is_some() {
                    states.push((file_state_id, "temporarily_unavailable"));
                }
            }
            states
        };
        let transaction = connection.transaction()?;
        let mut changed = 0_usize;
        for (file_state_id, availability) in changes {
            changed += transaction.execute(
                "UPDATE wallpaper_file_state SET
                    availability = ?2,
                    last_verified_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                    missing_since = CASE
                        WHEN ?2 = 'missing' THEN COALESCE(missing_since, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                        ELSE NULL
                    END
                 WHERE id = ?1 AND availability <> ?2",
                params![file_state_id, availability],
            )?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    /// Returns true and refreshes verification time when size and mtime prove a file unchanged.
    pub fn local_file_is_unchanged(
        &self,
        path: &Path,
        file_size: u64,
        modified_at_ms: u64,
    ) -> AppResult<bool> {
        let file_size = i64::try_from(file_size)
            .map_err(|_| AppError::FileSystem("local file size exceeds SQLite range".into()))?;
        let modified_at_ms = i64::try_from(modified_at_ms).map_err(|_| {
            AppError::FileSystem("local file timestamp exceeds SQLite range".into())
        })?;
        let connection = self.lock()?;
        let changed = connection.execute(
            "UPDATE wallpaper_file_state SET
                availability = 'available',
                last_verified_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                missing_since = NULL
             WHERE path = ?1 AND file_size = ?2 AND modified_at_ms = ?3
               AND storage_kind = 'user_source'",
            params![path.display().to_string(), file_size, modified_at_ms],
        )?;
        Ok(changed == 1)
    }

    /// Stores the cheap filesystem snapshot only after changed image content was validated.
    pub fn record_local_file_snapshot(
        &self,
        path: &Path,
        file_size: u64,
        modified_at_ms: u64,
    ) -> AppResult<()> {
        let file_size = i64::try_from(file_size)
            .map_err(|_| AppError::FileSystem("local file size exceeds SQLite range".into()))?;
        let modified_at_ms = i64::try_from(modified_at_ms).map_err(|_| {
            AppError::FileSystem("local file timestamp exceeds SQLite range".into())
        })?;
        let connection = self.lock()?;
        let changed = connection.execute(
            "UPDATE wallpaper_file_state SET
                file_size = ?2,
                modified_at_ms = ?3,
                availability = 'available',
                last_verified_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                missing_since = NULL
             WHERE path = ?1 AND storage_kind = 'user_source'",
            params![path.display().to_string(), file_size, modified_at_ms],
        )?;
        if changed != 1 {
            return Err(AppError::Database(format!(
                "local file state was not created for {}",
                path.display()
            )));
        }
        Ok(())
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
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE wallpaper SET local_path = NULL, hash = NULL, download_status = 'remote', downloaded_at = NULL
             WHERE provider <> 'local' AND local_path = ?1 AND favorite = 0",
            [path],
        )?;
        transaction.execute(
            "DELETE FROM wallpaper_file_state WHERE path = ?1 AND storage_kind = 'managed_download'",
            [path],
        )?;
        transaction.commit()?;
        Ok(changed)
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
        let strategy = if selection_mode == "random" {
            "shuffle"
        } else {
            "round_robin"
        };
        transaction.execute(
            "INSERT INTO monitor_rotation_policy(system_monitor_id, strategy, rules_json, updated_at)
             VALUES (?1, ?2, '{}', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(system_monitor_id) DO UPDATE SET
                strategy = excluded.strategy,
                updated_at = excluded.updated_at",
            params![system_monitor_id, strategy],
        )?;
        transaction.execute(
            "DELETE FROM rotation_queue WHERE system_monitor_id = ?1",
            [system_monitor_id],
        )?;
        transaction.commit()?;
        drop(connection);
        self.schedule_for_monitor(system_monitor_id)?
            .ok_or_else(|| AppError::Database("configured schedule could not be reloaded".into()))
    }

    /// Applies an advanced strategy and collection sources after candidates are materialized.
    pub fn set_rotation_policy(
        &self,
        system_monitor_id: &str,
        strategy: &str,
        collection_ids: &[i64],
        rules_json: &str,
    ) -> AppResult<()> {
        if !matches!(
            strategy,
            "round_robin" | "shuffle" | "least_recent" | "weighted_random"
        ) {
            return Err(AppError::Configuration(format!(
                "unsupported rotation strategy: {strategy}"
            )));
        }
        let _ = crate::models::RotationRules::from_json(rules_json)?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO monitor_rotation_policy(system_monitor_id, strategy, rules_json, updated_at)
             VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(system_monitor_id) DO UPDATE SET
                strategy = excluded.strategy,
                rules_json = excluded.rules_json,
                updated_at = excluded.updated_at",
            params![system_monitor_id, strategy, rules_json],
        )?;
        transaction.execute(
            "DELETE FROM monitor_rotation_source WHERE system_monitor_id = ?1",
            [system_monitor_id],
        )?;
        for collection_id in collection_ids.iter().copied() {
            transaction.execute(
                "INSERT INTO monitor_rotation_source(system_monitor_id, collection_id, weight, enabled)
                 VALUES (?1, ?2, 1, 1)",
                params![system_monitor_id, collection_id],
            )?;
        }
        transaction.execute(
            "DELETE FROM rotation_queue WHERE system_monitor_id = ?1",
            [system_monitor_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Reports the active strategy, last choice explanation, and queue/source sizes.
    pub fn rotation_explanation(
        &self,
        system_monitor_id: &str,
    ) -> AppResult<crate::models::RotationExplanation> {
        let connection = self.lock()?;
        connection
            .query_row(
                "SELECT policy.system_monitor_id, policy.strategy, policy.last_reason,
                        (SELECT COUNT(*) FROM monitor_rotation_source AS source
                         WHERE source.system_monitor_id = policy.system_monitor_id AND source.enabled = 1),
                        (SELECT GROUP_CONCAT(source.collection_id) FROM monitor_rotation_source AS source
                         WHERE source.system_monitor_id = policy.system_monitor_id AND source.enabled = 1),
                        (SELECT COUNT(*) FROM rotation_wallpaper AS candidate
                         JOIN wallpaper ON wallpaper.id = candidate.wallpaper_id
                         WHERE candidate.system_monitor_id = policy.system_monitor_id
                           AND wallpaper.blacklisted = 0),
                        (SELECT COUNT(*) FROM rotation_queue AS queue
                         WHERE queue.system_monitor_id = policy.system_monitor_id
                           AND queue.consumed_at IS NULL)
                 FROM monitor_rotation_policy AS policy WHERE policy.system_monitor_id = ?1",
                [system_monitor_id],
                |row| {
                    let source_count: i64 = row.get(3)?;
                    let source_ids: Option<String> = row.get(4)?;
                    let candidate_count: i64 = row.get(5)?;
                    let queued_count: i64 = row.get(6)?;
                    Ok(crate::models::RotationExplanation {
                        system_monitor_id: row.get(0)?,
                        strategy: row.get(1)?,
                        last_reason: row.get(2)?,
                        source_collection_count: u32::try_from(source_count).map_err(|_| {
                            rusqlite::Error::IntegralValueOutOfRange(3, source_count)
                        })?,
                        source_collection_ids: source_ids
                            .as_deref()
                            .unwrap_or_default()
                            .split(',')
                            .filter_map(|value| value.parse().ok())
                            .collect(),
                        candidate_count: u32::try_from(candidate_count).map_err(|_| {
                            rusqlite::Error::IntegralValueOutOfRange(5, candidate_count)
                        })?,
                        queued_count: u32::try_from(queued_count).map_err(|_| {
                            rusqlite::Error::IntegralValueOutOfRange(6, queued_count)
                        })?,
                    })
                },
            )
            .map_err(AppError::from)
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

    /// Loads validated per-monitor constraints, defaulting legacy policies to unrestricted rules.
    pub fn rotation_rules(
        &self,
        system_monitor_id: &str,
    ) -> AppResult<crate::models::RotationRules> {
        let connection = self.lock()?;
        let rules = connection
            .query_row(
                "SELECT rules_json FROM monitor_rotation_policy WHERE system_monitor_id = ?1",
                [system_monitor_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| {
                serde_json::to_string(&crate::models::RotationRules::default())
                    .unwrap_or_else(|_| "{}".into())
            });
        crate::models::RotationRules::from_json(&rules)
    }

    /// Defers a rule-paused schedule briefly without consuming a candidate or creating history.
    pub fn defer_schedule_for_rule(&self, system_monitor_id: &str, reason: &str) -> AppResult<()> {
        let connection = self.lock()?;
        connection.execute(
            "UPDATE monitor_schedule SET
                next_change_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+60 seconds'),
                last_error = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE system_monitor_id = ?1",
            params![
                system_monitor_id,
                reason.chars().take(500).collect::<String>()
            ],
        )?;
        Ok(())
    }

    /// Selects from the explicit pool, or falls back to five recent valid local originals.
    pub fn next_rotation_wallpaper(&self, system_monitor_id: &str) -> AppResult<WallpaperRecord> {
        let connection = self.lock()?;
        let schedule = connection
            .query_row(
                "SELECT COALESCE(
                            (SELECT strategy FROM monitor_rotation_policy AS policy
                             WHERE policy.system_monitor_id = monitor_schedule.system_monitor_id),
                            CASE WHEN selection_mode = 'random' THEN 'shuffle' ELSE 'round_robin' END
                        ),
                        (SELECT COUNT(*) FROM rotation_wallpaper
                         WHERE system_monitor_id = monitor_schedule.system_monitor_id)
                 FROM monitor_schedule WHERE system_monitor_id = ?1 AND enabled = 1",
                [system_monitor_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or_else(|| AppError::Configuration("enabled schedule does not exist".into()))?;

        if schedule.1 > 0 {
            if schedule.0 == "shuffle" {
                drop(connection);
                return self.next_shuffled_wallpaper(system_monitor_id);
            }
            let order = if schedule.0 == "weighted_random" {
                "CASE WHEN wallpaper.id = COALESCE((
                    SELECT history.wallpaper_id FROM wallpaper_history AS history
                    JOIN monitor ON monitor.id = history.monitor_id
                    WHERE monitor.system_monitor_id = ?1
                    ORDER BY history.used_at DESC, history.id DESC LIMIT 1
                 ), -1) THEN 1 ELSE 0 END,
                 ABS(RANDOM()) / CASE WHEN wallpaper.favorite = 1 THEN 2 ELSE 1 END"
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
            let reason = match schedule.0.as_str() {
                "weighted_random" => "加权随机：优先收藏并避免刚使用的壁纸",
                "least_recent" => "最近未使用优先：选择该屏最久未使用项",
                _ => "顺序轮询：按最近使用时间与集合顺序选择",
            };
            connection.execute(
                "UPDATE monitor_rotation_policy SET last_reason = ?2,
                        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE system_monitor_id = ?1",
                params![system_monitor_id, reason],
            )?;
            return load_wallpaper(&connection, wallpaper_id)?.ok_or_else(|| {
                AppError::Wallpaper("selected rotation wallpaper disappeared".into())
            });
        }

        let order = if matches!(schedule.0.as_str(), "shuffle" | "weighted_random") {
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

    /// Consumes a persisted random queue and rebuilds it only after one complete non-repeating round.
    fn next_shuffled_wallpaper(&self, system_monitor_id: &str) -> AppResult<WallpaperRecord> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let current_generation: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(generation), 0) FROM rotation_queue WHERE system_monitor_id = ?1",
            [system_monitor_id],
            |row| row.get(0),
        )?;
        let mut wallpaper_id = transaction
            .query_row(
                "SELECT queue.wallpaper_id FROM rotation_queue AS queue
                 JOIN wallpaper ON wallpaper.id = queue.wallpaper_id
                 WHERE queue.system_monitor_id = ?1 AND queue.generation = ?2
                   AND queue.consumed_at IS NULL AND wallpaper.blacklisted = 0
                 ORDER BY queue.position LIMIT 1",
                params![system_monitor_id, current_generation],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;

        if wallpaper_id.is_none() {
            let next_generation = current_generation.saturating_add(1);
            let mut candidates = {
                let mut statement = transaction.prepare(
                    "SELECT wallpaper.id FROM rotation_wallpaper AS rotation
                     JOIN wallpaper ON wallpaper.id = rotation.wallpaper_id
                     WHERE rotation.system_monitor_id = ?1 AND wallpaper.blacklisted = 0
                     ORDER BY RANDOM()",
                )?;
                let rows = statement.query_map([system_monitor_id], |row| row.get::<_, i64>(0))?;
                rows.collect::<Result<Vec<_>, _>>()?
            };
            if candidates.is_empty() {
                return Err(AppError::Wallpaper(
                    "selected rotation pool has no usable wallpaper".into(),
                ));
            }
            let previous = transaction
                .query_row(
                    "SELECT history.wallpaper_id FROM wallpaper_history AS history
                     JOIN monitor ON monitor.id = history.monitor_id
                     WHERE monitor.system_monitor_id = ?1
                     ORDER BY history.used_at DESC, history.id DESC LIMIT 1",
                    [system_monitor_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            if candidates.len() > 1 && previous == candidates.first().copied() {
                candidates.swap(0, 1);
            }
            for (position, candidate_id) in candidates.iter().copied().enumerate() {
                let position = i64::try_from(position).map_err(|_| {
                    AppError::Database("shuffle queue position exceeds SQLite range".into())
                })?;
                transaction.execute(
                    "INSERT INTO rotation_queue(system_monitor_id, generation, wallpaper_id, position)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![system_monitor_id, next_generation, candidate_id, position],
                )?;
            }
            transaction.execute(
                "DELETE FROM rotation_queue WHERE system_monitor_id = ?1 AND generation < ?2",
                params![system_monitor_id, next_generation],
            )?;
            wallpaper_id = candidates.first().copied();
        }

        let wallpaper_id = wallpaper_id.ok_or_else(|| {
            AppError::Wallpaper("shuffle queue could not select a wallpaper".into())
        })?;
        transaction.execute(
            "UPDATE rotation_queue SET consumed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE system_monitor_id = ?1 AND wallpaper_id = ?2 AND consumed_at IS NULL",
            params![system_monitor_id, wallpaper_id],
        )?;
        transaction.execute(
            "UPDATE monitor_rotation_policy SET
                last_reason = '洗牌队列：本轮未重复候选',
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE system_monitor_id = ?1",
            [system_monitor_id],
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_wallpaper(wallpaper_id)
    }

    /// Returns the most recent different wallpaper for user-driven Previous navigation.
    pub fn previous_rotation_wallpaper(
        &self,
        system_monitor_id: &str,
    ) -> AppResult<WallpaperRecord> {
        let connection = self.lock()?;
        let wallpaper_id = connection
            .query_row(
                "WITH ordered AS (
                    SELECT history.wallpaper_id,
                           ROW_NUMBER() OVER (ORDER BY history.used_at DESC, history.id DESC) AS row_number
                    FROM wallpaper_history AS history
                    JOIN monitor ON monitor.id = history.monitor_id
                    JOIN wallpaper ON wallpaper.id = history.wallpaper_id
                    WHERE monitor.system_monitor_id = ?1 AND wallpaper.blacklisted = 0
                 ), current AS (
                    SELECT wallpaper_id FROM ordered WHERE row_number = 1
                 )
                 SELECT ordered.wallpaper_id FROM ordered, current
                 WHERE ordered.wallpaper_id <> current.wallpaper_id
                 ORDER BY ordered.row_number LIMIT 1",
                [system_monitor_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or_else(|| AppError::Wallpaper("no previous wallpaper is available".into()))?;
        load_wallpaper(&connection, wallpaper_id)?
            .ok_or_else(|| AppError::Wallpaper("previous wallpaper no longer exists".into()))
    }

    /// Overrides the explanation after an explicit user control such as Skip or Previous.
    pub fn set_rotation_reason(&self, system_monitor_id: &str, reason: &str) -> AppResult<()> {
        let connection = self.lock()?;
        connection.execute(
            "UPDATE monitor_rotation_policy SET last_reason = ?2,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE system_monitor_id = ?1",
            params![system_monitor_id, reason],
        )?;
        Ok(())
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
        file_availability: row.get(25)?,
        storage_kind: row.get(26)?,
        file_copy_count: row.get(27)?,
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
                    preset, created_at, synced_at, downloaded_at, last_used_at,
                    COALESCE((SELECT state.availability FROM wallpaper_file_state AS state
                              WHERE state.wallpaper_id = wallpaper.id
                              ORDER BY CASE state.availability WHEN 'available' THEN 0 WHEN 'temporarily_unavailable' THEN 1 ELSE 2 END, state.id
                              LIMIT 1), 'remote') AS file_availability,
                    COALESCE((SELECT state.storage_kind FROM wallpaper_file_state AS state
                              WHERE state.wallpaper_id = wallpaper.id
                              ORDER BY CASE state.availability WHEN 'available' THEN 0 WHEN 'temporarily_unavailable' THEN 1 ELSE 2 END, state.id
                              LIMIT 1), 'remote_metadata') AS storage_kind,
                    (SELECT COUNT(*) FROM wallpaper_file_state AS state WHERE state.wallpaper_id = wallpaper.id) AS file_copy_count
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

/// Canonicalizes URL syntax and query ordering without discarding provider authorization data.
fn normalize_original_url(value: &str) -> Option<String> {
    let mut url = reqwest::Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    url.set_fragment(None);
    let mut query: Vec<(String, String)> = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    query.sort();
    url.set_query(None);
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query);
    }
    Some(url.to_string())
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
    use std::time::Instant;

    use rusqlite::{Connection, params};

    use super::{Database, MIGRATIONS};
    use crate::image_processing::ImageMetadata;
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
            perceptual_hash: None,
            download_status: "remote".into(),
            preset: true,
            created_at: Some("2026-08-20T00:00:00Z".into()),
            author: None,
            license_name: None,
            license_url: None,
            synced_at: "2026-08-20T00:00:00Z".into(),
            tags: vec!["mountain".into(), "lake".into()],
        }
    }

    #[test]
    fn migrations_are_idempotent() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("test.db");
        let first = Database::open(&path)?;
        assert_eq!(first.schema_version()?, 9);
        drop(first);
        let second = Database::open(&path)?;
        assert_eq!(second.schema_version()?, 9);
        Ok(())
    }

    #[test]
    fn v1_database_upgrades_without_losing_preferences_history_or_schedule()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("upgrade.db");
        let mut connection = Connection::open(&path)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE schema_migration(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);",
        )?;
        for (version, sql) in MIGRATIONS.iter().take(4) {
            let transaction = connection.transaction()?;
            transaction.execute_batch(sql)?;
            transaction.execute(
                "INSERT INTO schema_migration(version, applied_at) VALUES (?1, '2026-08-28T00:00:00Z')",
                [version],
            )?;
            transaction.commit()?;
        }
        connection.execute(
            "INSERT INTO wallpaper(provider, remote_id, name, width, height, category, purity,
                download_status, favorite, blacklisted, preset, synced_at)
             VALUES ('wallhaven', 'legacy', 'Legacy Favorite', 3840, 2160, 'nature', 'sfw',
                'remote', 1, 0, 0, '2026-08-28T00:00:00Z')",
            [],
        )?;
        let wallpaper_id = connection.last_insert_rowid();
        connection.execute(
            "INSERT INTO monitor(system_monitor_id, name, width, height, position_x, position_y,
                primary_display, last_seen_at)
             VALUES ('DISPLAY-1', 'Legacy Display', 1920, 1080, 0, 0, 1, '2026-08-28T00:00:00Z')",
            [],
        )?;
        let monitor_id = connection.last_insert_rowid();
        connection.execute(
            "INSERT INTO wallpaper_history(wallpaper_id, monitor_id, used_at, trigger_type)
             VALUES (?1, ?2, '2026-08-28T00:00:00Z', 'MANUAL')",
            params![wallpaper_id, monitor_id],
        )?;
        connection.execute(
            "INSERT INTO monitor_schedule(system_monitor_id, enabled, paused, interval_seconds,
                fit_mode, next_change_at, updated_at, selection_mode)
             VALUES ('DISPLAY-1', 1, 0, 1800, 'fill', '2026-08-28T00:00:00Z',
                '2026-08-28T00:00:00Z', 'random')",
            [],
        )?;
        drop(connection);

        let database = Database::open(&path)?;
        assert_eq!(database.schema_version()?, 9);
        let upgraded = database.list_wallpapers(1, 10, false)?;
        assert_eq!(upgraded.total, 1);
        assert!(upgraded.items[0].favorite);
        assert_eq!(database.list_schedules()?[0].selection_mode, "random");
        assert_eq!(database.list_provider_status()?.len(), 3);
        let connection = database.lock()?;
        assert_eq!(
            connection.query_row("SELECT COUNT(*) FROM wallpaper_history", [], |row| row
                .get::<_, i64>(0))?,
            1
        );
        Ok(())
    }

    #[test]
    fn ten_thousand_metadata_rows_keep_combined_query_below_target()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("performance.db"))?;
        let wallpapers = (0..10_000)
            .map(|index| {
                let mut wallpaper = sample_wallpaper();
                wallpaper.remote_id = format!("performance-{index}");
                wallpaper.name = format!("Landscape {index}");
                wallpaper.original_url = Some(format!("https://example.test/{index}.jpg"));
                wallpaper.preset = false;
                wallpaper
            })
            .collect::<Vec<_>>();
        database.upsert_wallpapers(&wallpapers)?;

        let started = Instant::now();
        let page = database.search_wallpapers(&CatalogQuery {
            keyword: Some("Landscape 99".into()),
            category: Some("nature".into()),
            provider: Some("wallhaven".into()),
            min_width: Some(3840),
            page: 1,
            page_size: 60,
            ..CatalogQuery::default()
        })?;
        let elapsed = started.elapsed();
        assert!(page.total > 0);
        assert!(elapsed.as_millis() < 200, "combined query took {elapsed:?}");
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
    fn canonical_url_dedup_keeps_all_provider_attribution() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        let first = sample_wallpaper();
        let mut second = sample_wallpaper();
        second.provider = "wikimedia_commons".into();
        second.remote_id = "File:Mountain.jpg".into();
        second.original_url =
            Some("https://w.wallhaven.cc/full/ab/wallhaven-abc123.jpg?b=2&a=1".into());
        let mut canonical_first = first;
        canonical_first.original_url =
            Some("https://w.wallhaven.cc/full/ab/wallhaven-abc123.jpg?a=1&b=2".into());

        database.upsert_wallpapers(&[canonical_first, second])?;

        let page = database.list_wallpapers(1, 10, false)?;
        assert_eq!(page.total, 1);
        assert_eq!(database.list_wallpaper_sources(page.items[0].id)?.len(), 2);
        Ok(())
    }

    #[test]
    fn content_hash_dedup_merges_sources_and_user_state() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        let first = sample_wallpaper();
        let mut second = sample_wallpaper();
        second.provider = "wikimedia_commons".into();
        second.remote_id = "File:Different-url.jpg".into();
        second.original_url = Some("https://upload.wikimedia.org/example.jpg".into());
        database.upsert_wallpapers(&[first, second])?;
        let page = database.list_wallpapers(1, 10, false)?;
        let second_id = page
            .items
            .iter()
            .find(|item| item.provider == "wikimedia_commons")
            .ok_or("second source fixture missing")?
            .id;
        let first_id = page
            .items
            .iter()
            .find(|item| item.provider == "wallhaven")
            .ok_or("first source fixture missing")?
            .id;
        database.set_wallpaper_favorite(second_id, true)?;
        let metadata = ImageMetadata {
            width: 3840,
            height: 2160,
            aspect_ratio: "16:9".into(),
            file_size: 128,
            mime_type: "image/jpeg",
            format: "jpeg",
            sha256: "same-content-hash".into(),
            perceptual_hash: "0011223344556677".into(),
        };
        database.mark_wallpaper_downloaded(
            first_id,
            &directory.path().join("first.jpg"),
            &metadata,
        )?;
        let retained = database.mark_wallpaper_downloaded(
            second_id,
            &directory.path().join("first.jpg"),
            &metadata,
        )?;

        assert_eq!(database.list_wallpapers(1, 10, false)?.total, 1);
        assert!(retained.favorite);
        assert_eq!(database.list_wallpaper_sources(retained.id)?.len(), 2);
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
        downloaded.original_url = Some("https://example.test/downloaded123.jpg".into());
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
    fn disliked_identity_suppresses_an_alternate_provider_url_match()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        let first = sample_wallpaper();
        database.upsert_wallpapers(std::slice::from_ref(&first))?;
        let wallpaper_id = database.list_wallpapers(1, 10, false)?.items[0].id;
        database.set_wallpaper_blacklisted(wallpaper_id, true)?;
        let mut alternate = first;
        alternate.provider = "wikimedia_commons".into();
        alternate.remote_id = "File:Same-image.jpg".into();
        database.upsert_wallpapers(&[alternate])?;

        assert_eq!(database.list_wallpapers(1, 10, false)?.total, 0);
        assert_eq!(database.list_wallpaper_sources(wallpaper_id)?.len(), 2);
        Ok(())
    }

    #[test]
    fn perceptual_identity_merges_alternate_urls_and_preserves_dislike()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        let mut first = sample_wallpaper();
        first.perceptual_hash = Some("0123456789abcdeffedcba9876543210".into());
        database.upsert_wallpapers(std::slice::from_ref(&first))?;
        let wallpaper_id = database.list_wallpapers(1, 10, false)?.items[0].id;
        database.set_wallpaper_blacklisted(wallpaper_id, true)?;

        let mut alternate = first;
        alternate.provider = "wikimedia_commons".into();
        alternate.remote_id = "File:Reencoded-image.jpg".into();
        alternate.original_url = Some("https://upload.example.invalid/reencoded.jpg".into());
        database.upsert_wallpapers(&[alternate])?;

        assert_eq!(database.list_wallpapers(1, 10, false)?.total, 0);
        assert_eq!(database.list_wallpaper_sources(wallpaper_id)?.len(), 2);
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
        valid.original_url = Some("https://example.test/valid.jpg".into());
        valid.local_path = Some(valid_path.display().to_string());
        valid.download_status = "downloaded".into();
        let mut blacklisted = valid.clone();
        blacklisted.remote_id = "blacklisted".into();
        blacklisted.original_url = Some("https://example.test/blacklisted.jpg".into());
        blacklisted.local_path = Some(blacklisted_path.display().to_string());
        let mut deleted = valid.clone();
        deleted.remote_id = "deleted".into();
        deleted.original_url = Some("https://example.test/deleted.jpg".into());
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
        second.original_url = Some("https://example.test/second.jpg".into());
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
    fn gallery_refresh_marks_deleted_local_files_without_losing_history()
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
        existing.original_url = None;
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

        assert_eq!(
            database.reconcile_local_file_states(&[directory.path().display().to_string()])?,
            1
        );
        assert_eq!(
            database.get_wallpaper(deleted_id)?.file_availability,
            "missing"
        );
        assert!(existing_path.is_file());
        Ok(())
    }

    #[test]
    fn local_snapshot_skips_unchanged_files_and_detects_changes()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        let path = directory.path().join("indexed.jpg");
        std::fs::write(&path, b"fixture")?;
        let mut wallpaper = sample_wallpaper();
        wallpaper.provider = "local".into();
        wallpaper.remote_id = "local-snapshot".into();
        wallpaper.local_path = Some(path.display().to_string());
        wallpaper.hash = Some("local-snapshot".into());
        wallpaper.download_status = "downloaded".into();
        database.upsert_wallpapers(&[wallpaper])?;
        database.record_local_file_snapshot(&path, 7, 1234)?;

        assert!(database.local_file_is_unchanged(&path, 7, 1234)?);
        assert!(!database.local_file_is_unchanged(&path, 8, 1234)?);
        Ok(())
    }

    #[test]
    fn duplicate_view_lists_every_path_for_one_content_hash()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        let mut first = sample_wallpaper();
        first.provider = "local".into();
        first.remote_id = "same-content".into();
        first.hash = Some("same-content".into());
        first.local_path = Some(directory.path().join("first.jpg").display().to_string());
        first.download_status = "downloaded".into();
        let mut second = first.clone();
        second.local_path = Some(directory.path().join("second.jpg").display().to_string());

        database.upsert_wallpapers(&[first])?;
        database.upsert_wallpapers(&[second])?;

        let groups = database.list_duplicate_file_groups()?;
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].copies.len(), 2);
        Ok(())
    }

    #[test]
    fn offline_scan_root_marks_files_temporarily_unavailable()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        let offline_root = directory.path().join("detached-drive");
        let mut wallpaper = sample_wallpaper();
        wallpaper.provider = "local".into();
        wallpaper.remote_id = "offline-local".into();
        wallpaper.local_path = Some(offline_root.join("photo.jpg").display().to_string());
        wallpaper.download_status = "downloaded".into();
        database.upsert_wallpapers(&[wallpaper])?;

        assert_eq!(
            database.reconcile_local_file_states(&[offline_root.display().to_string()])?,
            1
        );
        let record = database.list_wallpapers(1, 10, false)?.items.remove(0);
        assert_eq!(record.file_availability, "temporarily_unavailable");
        Ok(())
    }
}
