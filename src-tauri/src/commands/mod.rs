use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tauri::State;
use tauri_plugin_autostart::ManagerExt;

use crate::{
    AppState,
    cache::{CacheCleanupResult, CacheInfo, CacheService},
    error::AppResult,
    image_processing::{FitMode, ImageMetadata, ProcessedImage},
    models::{
        CatalogQuery, MonitorInfo, NewWallpaper, ScheduleRecord, WallpaperPage, WallpaperRecord,
    },
    provider::{LocalProvider, RemoteWallpaper, WallpaperCategory, WallpaperQuery},
    settings::AppConfig,
    wallpaper::WallpaperService,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    app_data_directory: String,
    database_path: String,
    platform: &'static str,
    schema_version: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailData {
    mime_type: &'static str,
    bytes: Vec<u8>,
}

/// Reports initialized Phase 0 resources to the lightweight validation UI.
#[tauri::command]
pub fn get_app_status(state: State<'_, AppState>) -> AppResult<AppStatus> {
    Ok(AppStatus {
        app_data_directory: state.paths.root.display().to_string(),
        database_path: state.paths.database_file.display().to_string(),
        platform: state.platform.platform_name,
        schema_version: state.database.schema_version()?,
    })
}

/// Returns a fresh platform-native monitor snapshot.
#[tauri::command]
pub fn get_monitors(state: State<'_, AppState>) -> AppResult<Vec<MonitorInfo>> {
    let monitors = state.platform.monitors.get_monitors()?;
    tracing::info!(count = monitors.len(), "display enumeration completed");
    Ok(monitors)
}

/// Sets one validated local image on every active display.
#[tauri::command]
pub async fn set_wallpaper(path: String, state: State<'_, AppState>) -> AppResult<()> {
    validate_image_before_platform_call(&path, &state).await?;
    state
        .platform
        .wallpaper
        .set_wallpaper_for_all(path.as_ref())
}

/// Sets one validated local image on a specific native monitor identifier.
#[tauri::command]
pub async fn set_wallpaper_for_monitor(
    path: String,
    monitor_id: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    validate_image_before_platform_call(&path, &state).await?;
    state
        .platform
        .wallpaper
        .set_wallpaper_for_monitor(&monitor_id, path.as_ref())
}

/// Decodes selected input off the UI thread before any native wallpaper API receives it.
async fn validate_image_before_platform_call(
    path: &str,
    state: &State<'_, AppState>,
) -> AppResult<()> {
    let processor = state.images.clone();
    let path = std::path::PathBuf::from(path);
    tokio::task::spawn_blocking(move || processor.inspect(&path))
        .await
        .map_err(|error| crate::error::AppError::Image(format!("image task failed: {error}")))??;
    Ok(())
}

/// Lists catalog metadata with a hard page-size ceiling enforced by the database.
#[tauri::command]
pub fn list_wallpapers(
    page: u32,
    page_size: u32,
    preset_only: bool,
    state: State<'_, AppState>,
) -> AppResult<WallpaperPage> {
    state.database.list_wallpapers(page, page_size, preset_only)
}

/// Reads one small bundled thumbnail after enforcing the AppData cache boundary.
#[tauri::command]
pub fn get_wallpaper_thumbnail(
    wallpaper_id: i64,
    state: State<'_, AppState>,
) -> AppResult<ThumbnailData> {
    let path = state
        .database
        .thumbnail_path(wallpaper_id)?
        .ok_or_else(|| crate::error::AppError::Image("thumbnail is not cached".into()))?;
    let canonical_cache = std::fs::canonicalize(&state.paths.thumbnails_dir)?;
    let canonical_thumbnail = std::fs::canonicalize(&path)?;
    if !canonical_thumbnail.starts_with(canonical_cache) {
        return Err(crate::error::AppError::Image(
            "thumbnail path is outside the application cache".into(),
        ));
    }
    let mime_type = match canonical_thumbnail
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        _ => "image/jpeg",
    };
    Ok(ThumbnailData {
        mime_type,
        bytes: std::fs::read(canonical_thumbnail)?,
    })
}

/// Decodes one explicitly selected image on a blocking worker and returns trusted metadata.
#[tauri::command]
pub async fn inspect_image_file(
    path: String,
    state: State<'_, AppState>,
) -> AppResult<ImageMetadata> {
    let processor = state.images.clone();
    tokio::task::spawn_blocking(move || processor.inspect(std::path::Path::new(&path)))
        .await
        .map_err(|error| crate::error::AppError::Image(format!("image task failed: {error}")))?
}

/// Generates or reuses a small proportional JPEG for metadata-first browsing.
#[tauri::command]
pub async fn create_thumbnail(
    path: String,
    max_width: Option<u32>,
    max_height: Option<u32>,
    state: State<'_, AppState>,
) -> AppResult<ProcessedImage> {
    let processor = state.images.clone();
    tokio::task::spawn_blocking(move || {
        processor.create_thumbnail(std::path::Path::new(&path), max_width, max_height)
    })
    .await
    .map_err(|error| crate::error::AppError::Image(format!("image task failed: {error}")))?
}

/// Uses the unified monitor adapter to render one display-specific processed wallpaper.
#[tauri::command]
pub async fn prepare_wallpaper_for_monitor(
    path: String,
    monitor_id: String,
    fit_mode: FitMode,
    state: State<'_, AppState>,
) -> AppResult<ProcessedImage> {
    let monitor = state
        .platform
        .monitors
        .get_monitors()?
        .into_iter()
        .find(|monitor| monitor.system_monitor_id == monitor_id)
        .ok_or_else(|| crate::error::AppError::Monitor("selected monitor is not active".into()))?;
    let processor = state.images.clone();
    tokio::task::spawn_blocking(move || {
        processor.prepare_for_display(
            std::path::Path::new(&path),
            monitor.width,
            monitor.height,
            fit_mode,
        )
    })
    .await
    .map_err(|error| crate::error::AppError::Image(format!("image task failed: {error}")))?
}

/// Fetches the newest provider-neutral metadata without coupling the UI to Wallhaven flags.
#[tauri::command]
pub async fn provider_latest(
    provider: String,
    query: WallpaperQuery,
    state: State<'_, AppState>,
) -> AppResult<Vec<RemoteWallpaper>> {
    state.providers.get(&provider)?.latest(query).await
}

/// Searches either V1 provider through the same query contract.
#[tauri::command]
pub async fn provider_search(
    provider: String,
    query: WallpaperQuery,
    state: State<'_, AppState>,
) -> AppResult<Vec<RemoteWallpaper>> {
    state.providers.get(&provider)?.search(query).await
}

/// Loads provider-specific detail and returns only the unified metadata model.
#[tauri::command]
pub async fn provider_detail(
    provider: String,
    remote_id: String,
    state: State<'_, AppState>,
) -> AppResult<RemoteWallpaper> {
    state.providers.get(&provider)?.get_detail(&remote_id).await
}

/// Downloads an original only after an explicit UI command requests it.
#[tauri::command]
pub async fn provider_download(
    provider: String,
    wallpaper: RemoteWallpaper,
    state: State<'_, AppState>,
) -> AppResult<String> {
    Ok(state
        .providers
        .get(&provider)?
        .download(&wallpaper)
        .await?
        .display()
        .to_string())
}

/// Downloads and deduplicates one catalog original, returning refreshed persisted metadata.
#[tauri::command]
pub async fn download_wallpaper(
    wallpaper_id: i64,
    state: State<'_, AppState>,
) -> AppResult<WallpaperRecord> {
    let wallpaper = wallpaper_service(&state)
        .ensure_original(wallpaper_id)
        .await?;
    enforce_cache_best_effort(&state);
    Ok(wallpaper)
}

/// Returns raw original bytes efficiently after the download command has completed.
#[tauri::command]
pub fn get_wallpaper_original_bytes(
    wallpaper_id: i64,
    state: State<'_, AppState>,
) -> AppResult<tauri::ipc::Response> {
    let wallpaper = state.database.get_wallpaper(wallpaper_id)?;
    let path = wallpaper
        .local_path
        .ok_or_else(|| crate::error::AppError::Wallpaper("original is not downloaded".into()))?;
    state.images.inspect(std::path::Path::new(&path))?;
    Ok(tauri::ipc::Response::new(std::fs::read(path)?))
}

/// Downloads, adapts, applies, and records one manual catalog wallpaper change.
#[tauri::command]
pub async fn apply_catalog_wallpaper(
    wallpaper_id: i64,
    monitor_id: String,
    fit_mode: FitMode,
    state: State<'_, AppState>,
) -> AppResult<ProcessedImage> {
    let processed = wallpaper_service(&state)
        .apply_to_monitor(wallpaper_id, &monitor_id, fit_mode, true)
        .await?;
    enforce_cache_best_effort(&state);
    Ok(processed)
}

/// Updates favorite state through the Phase 5 Core.
#[tauri::command]
pub fn set_wallpaper_favorite(
    wallpaper_id: i64,
    favorite: bool,
    state: State<'_, AppState>,
) -> AppResult<WallpaperRecord> {
    wallpaper_service(&state).set_favorite(wallpaper_id, favorite)
}

/// Updates blacklist state and removes blacklisted items from rotation pools.
#[tauri::command]
pub fn set_wallpaper_blacklisted(
    wallpaper_id: i64,
    blacklisted: bool,
    state: State<'_, AppState>,
) -> AppResult<WallpaperRecord> {
    wallpaper_service(&state).set_blacklisted(wallpaper_id, blacklisted)
}

/// Deletes one non-favorite remote original only inside the application-owned cache.
#[tauri::command]
pub fn delete_wallpaper_cache(
    wallpaper_id: i64,
    state: State<'_, AppState>,
) -> AppResult<WallpaperRecord> {
    let wallpaper = state.database.get_wallpaper(wallpaper_id)?;
    if wallpaper.favorite {
        return Err(crate::error::AppError::Wallpaper(
            "favorite originals are protected from cache deletion".into(),
        ));
    }
    if wallpaper.provider == "local" {
        return Err(crate::error::AppError::Wallpaper(
            "LocalProvider originals are user-owned and cannot be deleted".into(),
        ));
    }
    let Some(path_text) = wallpaper.local_path.as_deref() else {
        return Ok(wallpaper);
    };
    let canonical_root = std::fs::canonicalize(&state.paths.wallpapers_original_dir)?;
    let canonical_file = std::fs::canonicalize(path_text)?;
    if !canonical_file.starts_with(&canonical_root) {
        return Err(crate::error::AppError::FileSystem(
            "wallpaper cache path is outside the application original directory".into(),
        ));
    }
    let references = state
        .database
        .other_path_references(wallpaper_id, path_text)?;
    let updated = state.database.clear_wallpaper_download(wallpaper_id)?;
    if references == 0 {
        std::fs::remove_file(canonical_file)?;
    }
    Ok(updated)
}

/// Replaces the selected pool for one active monitor and starts with an immediate run.
#[tauri::command]
pub fn configure_wallpaper_rotation(
    monitor_id: String,
    wallpaper_ids: Vec<i64>,
    interval_seconds: u64,
    fit_mode: FitMode,
    selection_mode: String,
    state: State<'_, AppState>,
) -> AppResult<ScheduleRecord> {
    let monitors = state.platform.monitors.get_monitors()?;
    if !monitors
        .iter()
        .any(|monitor| monitor.system_monitor_id == monitor_id)
    {
        return Err(crate::error::AppError::Monitor(
            "selected monitor is not active".into(),
        ));
    }
    state.database.upsert_monitors(&monitors)?;
    let schedule = state.database.configure_rotation(
        &monitor_id,
        &wallpaper_ids,
        interval_seconds,
        fit_mode.as_str(),
        &selection_mode,
    )?;
    state.scheduler.wake();
    Ok(schedule)
}

/// Returns all persisted per-monitor scheduler states.
#[tauri::command]
pub fn get_scheduler_status(state: State<'_, AppState>) -> AppResult<Vec<ScheduleRecord>> {
    state.database.list_schedules()
}

/// Pauses one monitor's automatic rotation without discarding its selected pool.
#[tauri::command]
pub fn pause_scheduler(
    monitor_id: String,
    state: State<'_, AppState>,
) -> AppResult<ScheduleRecord> {
    state.database.set_schedule_paused(&monitor_id, true)
}

/// Resumes one monitor and requests a single immediate catch-up change.
#[tauri::command]
pub fn resume_scheduler(
    monitor_id: String,
    state: State<'_, AppState>,
) -> AppResult<ScheduleRecord> {
    let schedule = state.database.set_schedule_paused(&monitor_id, false)?;
    state.scheduler.wake();
    Ok(schedule)
}

/// Requests the next selected wallpaper immediately for one configured monitor.
#[tauri::command]
pub fn trigger_next_wallpaper(monitor_id: String, state: State<'_, AppState>) -> AppResult<()> {
    state.database.trigger_schedule_now(&monitor_id)?;
    state.scheduler.wake();
    Ok(())
}

/// Returns a bounded SQLite metadata page for Discover, Categories, Search, and Favorites.
#[tauri::command]
pub fn query_catalog(query: CatalogQuery, state: State<'_, AppState>) -> AppResult<WallpaperPage> {
    state.database.search_wallpapers(&query)
}

/// Fetches one Wallhaven metadata page and upserts it without downloading 4K originals.
#[tauri::command]
pub async fn sync_catalog(query: WallpaperQuery, state: State<'_, AppState>) -> AppResult<usize> {
    sync_online_metadata(query, &state).await
}

/// Performs the startup sync only when the last successful metadata refresh is stale.
#[tauri::command]
pub async fn sync_catalog_if_due(state: State<'_, AppState>) -> AppResult<usize> {
    let now = current_unix_seconds()?;
    let last_success = state
        .database
        .get_setting("resource_sync_last_success")?
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let settings = state
        .settings
        .lock()
        .map_err(|_| crate::error::AppError::configuration("settings mutex was poisoned"))?
        .clone();
    if !settings.resource_sync_enabled
        || now.saturating_sub(last_success) < settings.resource_sync_interval_seconds
    {
        return Ok(0);
    }
    sync_online_metadata(WallpaperQuery::default(), &state).await
}

/// Shares the provider-to-SQLite sync path between startup and manual refresh.
async fn sync_online_metadata(
    query: WallpaperQuery,
    state: &State<'_, AppState>,
) -> AppResult<usize> {
    let requested_category = query.category.clone();
    let requested_keyword = query
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let wallpapers = state.providers.get("wallhaven")?.search(query).await?;
    let synced_at = current_sync_stamp()?;
    let records: Vec<_> = wallpapers
        .into_iter()
        .map(|wallpaper| {
            let mut record = remote_to_new(wallpaper, &requested_category, &synced_at, None);
            if let Some(keyword) = requested_keyword.as_ref()
                && !record
                    .tags
                    .iter()
                    .any(|tag| tag.eq_ignore_ascii_case(keyword))
            {
                // Wallhaven search listings omit full tags; retain the proven query association.
                record.tags.push(keyword.clone());
            }
            record
        })
        .collect();
    let imported = state.database.upsert_wallpapers(&records)?;
    state.database.set_setting(
        "resource_sync_last_success",
        &serde_json::Value::from(current_unix_seconds()?),
    )?;
    tracing::info!(count = imported, "online wallpaper metadata synchronized");
    Ok(imported)
}

/// Scans one explicitly selected directory, creates thumbnails, and indexes original files in place.
#[tauri::command]
pub async fn scan_local_directory(path: String, state: State<'_, AppState>) -> AppResult<usize> {
    let root = std::fs::canonicalize(PathBuf::from(path))?;
    if !root.is_dir() {
        return Err(crate::error::AppError::Provider(
            "selected local path is not a directory".into(),
        ));
    }
    let imported = index_local_paths(vec![root.clone()], &state).await?;
    track_local_directories(&state, &[root])?;
    Ok(imported)
}

/// Imports dropped files or directories through the same LocalProvider validation pipeline.
#[tauri::command]
pub async fn import_local_paths(
    paths: Vec<String>,
    state: State<'_, AppState>,
) -> AppResult<usize> {
    if paths.is_empty() {
        return Ok(0);
    }
    let canonical_paths = paths
        .into_iter()
        .map(PathBuf::from)
        .map(std::fs::canonicalize)
        .collect::<Result<Vec<_>, _>>()?;
    let imported = index_local_paths(canonical_paths.clone(), &state).await?;
    let directories: Vec<_> = canonical_paths
        .into_iter()
        .filter(|path| path.is_dir())
        .collect();
    track_local_directories(&state, &directories)?;
    Ok(imported)
}

/// Removes one local catalog entry while preserving the user-owned source file.
#[tauri::command]
pub fn remove_local_wallpaper(wallpaper_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    state.database.remove_local_wallpaper_index(wallpaper_id)
}

/// Reconciles external file deletions before the refreshed gallery query runs.
#[tauri::command]
pub fn prune_missing_local_wallpapers(state: State<'_, AppState>) -> AppResult<usize> {
    state.database.prune_missing_local_wallpapers()
}

/// Validates, thumbnails, and indexes a bounded set of explicitly supplied local paths.
async fn index_local_paths(paths: Vec<PathBuf>, state: &State<'_, AppState>) -> AppResult<usize> {
    let provider = LocalProvider::new(paths);
    let wallpapers = provider.scan_all().await?;
    let synced_at = current_sync_stamp()?;
    let mut records = Vec::with_capacity(wallpapers.len());
    for wallpaper in wallpapers {
        let local_path = wallpaper.local_path.as_deref().ok_or_else(|| {
            crate::error::AppError::Provider("local scan returned no file path".into())
        })?;
        let thumbnail = state
            .images
            .create_thumbnail(local_path, Some(640), Some(360))?;
        records.push(remote_to_new(
            wallpaper,
            &WallpaperCategory::Local,
            &synced_at,
            Some(thumbnail.path),
        ));
    }
    state.database.upsert_wallpapers(&records)
}

/// Persists only dropped/scanned directories so individual files do not broaden disk access.
fn track_local_directories(state: &State<'_, AppState>, roots: &[PathBuf]) -> AppResult<()> {
    let mut settings = state
        .settings
        .lock()
        .map_err(|_| crate::error::AppError::configuration("settings mutex was poisoned"))?;
    let mut changed = false;
    for root in roots {
        let root_text = root.display().to_string();
        if !settings.local_directories.contains(&root_text) {
            settings.local_directories.push(root_text);
            changed = true;
        }
    }
    if changed {
        settings.local_directories.sort();
        settings.save(&state.paths.config_file)?;
    }
    Ok(())
}

/// Stops tracking a local root without modifying or deleting any user-owned image files.
#[tauri::command]
pub fn remove_local_directory(path: String, state: State<'_, AppState>) -> AppResult<AppConfig> {
    let mut settings = state
        .settings
        .lock()
        .map_err(|_| crate::error::AppError::configuration("settings mutex was poisoned"))?;
    settings.local_directories.retain(|item| item != &path);
    settings.save(&state.paths.config_file)?;
    Ok(settings.clone())
}

/// Returns the active local configuration used by the settings page.
#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<AppConfig> {
    state
        .settings
        .lock()
        .map(|settings| settings.clone())
        .map_err(|_| crate::error::AppError::configuration("settings mutex was poisoned"))
}

/// Validates and atomically stores editable V1 settings.
#[tauri::command]
pub fn update_settings(
    mut settings: AppConfig,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<AppConfig> {
    if settings.resource_sync_interval_seconds < 3600 {
        return Err(crate::error::AppError::configuration(
            "resource sync interval must be at least one hour",
        ));
    }
    if settings.wallpaper_change_interval_seconds < 60 {
        return Err(crate::error::AppError::configuration(
            "wallpaper interval must be at least one minute",
        ));
    }
    if !matches!(
        settings.wallpaper_fit_mode.as_str(),
        "fill" | "fit" | "center" | "stretch"
    ) {
        return Err(crate::error::AppError::configuration(
            "unsupported wallpaper fit mode",
        ));
    }
    if !matches!(
        settings.theme_mode.as_str(),
        "dark" | "light" | "system" | "custom"
    ) {
        return Err(crate::error::AppError::configuration(
            "unsupported application theme mode",
        ));
    }
    if !matches!(
        settings.theme_effect.as_str(),
        "solid" | "gradient" | "rainbow"
    ) {
        return Err(crate::error::AppError::configuration(
            "unsupported application theme effect",
        ));
    }
    // V1 system mode follows only OS light/dark appearance and never persists a custom background effect.
    if settings.theme_mode == "system" {
        settings.theme_effect = "solid".into();
    }
    if [
        &settings.theme_accent,
        &settings.theme_secondary,
        &settings.theme_background,
        &settings.theme_surface,
    ]
    .into_iter()
    .any(|color| !is_hex_color(color))
    {
        return Err(crate::error::AppError::configuration(
            "custom theme colors must use #RRGGBB format",
        ));
    }
    let autostart = app.autolaunch();
    if settings.auto_start {
        autostart.enable().map_err(|error| {
            crate::error::AppError::configuration(format!("auto start update failed: {error}"))
        })?;
    } else {
        // Disabling an entry that does not exist is idempotently equivalent to disabled.
        if let Err(error) = autostart.disable() {
            tracing::debug!(%error, "auto start was already disabled or unavailable");
        }
    }
    settings.save(&state.paths.config_file)?;
    let mut active = state
        .settings
        .lock()
        .map_err(|_| crate::error::AppError::configuration("settings mutex was poisoned"))?;
    *active = settings.clone();
    drop(active);
    enforce_cache_best_effort(&state);
    Ok(settings)
}

/// Accepts only fixed-width CSS hexadecimal colors before persisting user-controlled tokens.
fn is_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value.as_bytes()[1..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit())
}

/// Reports cache categories and the configured finite or unlimited capacity.
#[tauri::command]
pub fn get_cache_info(state: State<'_, AppState>) -> AppResult<CacheInfo> {
    let limit = cache_limit(&state)?;
    CacheService::new(state.database.clone(), state.paths.clone()).info(limit)
}

/// Removes processed files, LRU non-favorite originals, then thumbnails within owned paths.
#[tauri::command]
pub fn clear_cache(state: State<'_, AppState>) -> AppResult<CacheCleanupResult> {
    let limit = cache_limit(&state)?;
    CacheService::new(state.database.clone(), state.paths.clone()).clear_removable(limit)
}

/// Enforces cache limits without turning a successful wallpaper action into a UI failure.
fn enforce_cache_best_effort(state: &State<'_, AppState>) {
    let result = cache_limit(state).and_then(|limit| {
        CacheService::new(state.database.clone(), state.paths.clone()).enforce_limit(limit)
    });
    if let Err(error) = result {
        tracing::warn!(%error, "automatic cache cleanup failed");
    }
}

fn cache_limit(state: &State<'_, AppState>) -> AppResult<u64> {
    state
        .settings
        .lock()
        .map(|settings| settings.cache_limit_bytes)
        .map_err(|_| crate::error::AppError::configuration("settings mutex was poisoned"))
}

/// Maps provider metadata into the persisted catalog while preserving local originals in place.
fn remote_to_new(
    wallpaper: RemoteWallpaper,
    requested_category: &WallpaperCategory,
    synced_at: &str,
    thumbnail_local_path: Option<String>,
) -> NewWallpaper {
    let is_local = wallpaper.provider == "local";
    let category = match requested_category {
        WallpaperCategory::Nature => "nature".to_owned(),
        WallpaperCategory::Anime => "anime".to_owned(),
        WallpaperCategory::People => "people".to_owned(),
        WallpaperCategory::Local => "local".to_owned(),
        WallpaperCategory::All => match wallpaper.category.to_ascii_lowercase().as_str() {
            "anime" => "anime".to_owned(),
            "people" => "people".to_owned(),
            _ => "nature".to_owned(),
        },
    };
    NewWallpaper {
        provider: wallpaper.provider,
        remote_id: wallpaper.remote_id.clone(),
        name: wallpaper.name,
        source_page_url: wallpaper.source_page_url,
        original_url: wallpaper.original_url,
        thumbnail_url: wallpaper.thumbnail_url,
        thumbnail_local_path,
        local_path: wallpaper.local_path.map(|path| path.display().to_string()),
        width: wallpaper.width.unwrap_or(0),
        height: wallpaper.height.unwrap_or(0),
        aspect_ratio: wallpaper.ratio,
        file_size: wallpaper.file_size,
        mime_type: wallpaper.mime_type,
        category,
        purity: wallpaper.purity,
        hash: is_local.then_some(wallpaper.remote_id),
        download_status: if is_local { "downloaded" } else { "remote" }.into(),
        preset: false,
        created_at: wallpaper.created_at,
        synced_at: synced_at.to_owned(),
        tags: wallpaper.tags,
    }
}

/// Creates a fixed-width monotonic stamp without introducing a date-time dependency.
fn current_sync_stamp() -> AppResult<String> {
    Ok(format!("unix:{:020}", current_unix_seconds()?))
}

/// Returns seconds used solely for comparing the configured synchronization interval.
fn current_unix_seconds() -> AppResult<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| crate::error::AppError::unknown(error.to_string()))?
        .as_secs())
}

/// Constructs the Phase 5 service from Tauri-managed application resources.
fn wallpaper_service<'a>(state: &'a State<'_, AppState>) -> WallpaperService<'a> {
    WallpaperService::new(
        &state.database,
        &state.providers,
        &state.images,
        &state.platform,
        &state.paths.wallpapers_original_dir,
    )
}
