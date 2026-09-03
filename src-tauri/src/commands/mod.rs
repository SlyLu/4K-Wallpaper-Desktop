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
    image_processing::{FitMode, ImageMetadata, ProcessedImage, SpanFitMode},
    models::{
        AppliedWallpaper, CatalogQuery, CollectionRecord, DuplicateFileGroup, MonitorInfo,
        NewWallpaper, ProviderStatus, RotationExplanation, ScheduleRecord, SmartCollectionRule,
        WallpaperPage, WallpaperProviderSource, WallpaperRecord,
    },
    provider::{
        AggregatedProviderService, LocalProvider, ProviderServices, RemoteWallpaper,
        WallpaperCategory, WallpaperQuery,
    },
    settings::AppConfig,
    span::{MonitorLayout, SpannedWallpaperService, SpanningApplyResult, calculate_monitor_layout},
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

/// Processed local UI background data never leaves the Tauri IPC boundary or AppData.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeBackgroundData {
    pub path: String,
    pub mime_type: String,
    pub luminance: f32,
    pub bytes: Vec<u8>,
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

/// Returns a normalized virtual canvas used by preview and static spanning workflows.
#[tauri::command]
pub fn get_monitor_layout(state: State<'_, AppState>) -> AppResult<MonitorLayout> {
    let monitors = state.platform.monitors.get_monitors()?;
    calculate_monitor_layout(&monitors)
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
) -> AppResult<AppliedWallpaper> {
    let processed = wallpaper_service(&state)
        .apply_to_monitor(wallpaper_id, &monitor_id, fit_mode, true)
        .await?;
    enforce_cache_best_effort(&state);
    let wallpaper = state.database.wallpaper_by_hash(&processed.source_sha256)?;
    Ok(AppliedWallpaper {
        processed,
        wallpaper,
    })
}

/// Generates and applies one continuous static image across the current monitor geometry.
#[tauri::command]
pub async fn apply_spanning_wallpaper(
    wallpaper_id: i64,
    fit_mode: SpanFitMode,
    state: State<'_, AppState>,
) -> AppResult<SpanningApplyResult> {
    spanning_service(&state).apply(wallpaper_id, fit_mode).await
}

/// Restores the independent per-monitor paths captured before spanning mode was enabled.
#[tauri::command]
pub fn disable_spanning_wallpaper(state: State<'_, AppState>) -> AppResult<usize> {
    spanning_service(&state).disable()
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
    rules: Option<serde_json::Value>,
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
    if let Some(rules) = rules {
        let strategy = if selection_mode == "random" {
            "shuffle"
        } else {
            "round_robin"
        };
        state.database.set_rotation_policy(
            &monitor_id,
            strategy,
            &[],
            &serde_json::to_string(&rules)?,
        )?;
    }
    state.scheduler.wake();
    Ok(schedule)
}

/// Restores the durable catalog checkmarks used before the recent-five fallback.
#[tauri::command]
pub fn get_rotation_selection(state: State<'_, AppState>) -> AppResult<Vec<i64>> {
    state.database.rotation_selection_ids()
}

/// Persists one catalog checkmark immediately so sleep or restart cannot discard it.
#[tauri::command]
pub fn set_rotation_selection(
    wallpaper_id: i64,
    selected: bool,
    state: State<'_, AppState>,
) -> AppResult<Vec<i64>> {
    state
        .database
        .set_rotation_selection(wallpaper_id, selected)
}

/// Returns validated persisted rules so monitor forms never reset on navigation.
#[tauri::command]
pub fn get_rotation_rules(
    monitor_id: String,
    state: State<'_, AppState>,
) -> AppResult<crate::models::RotationRules> {
    state.database.rotation_rules(&monitor_id)
}

/// Configures collection-backed V2 rotation while preserving the V1 scheduler contract.
#[tauri::command]
pub fn configure_rotation_policy(
    monitor_id: String,
    collection_ids: Vec<i64>,
    interval_seconds: u64,
    fit_mode: String,
    strategy: String,
    rules: serde_json::Value,
    state: State<'_, AppState>,
) -> AppResult<ScheduleRecord> {
    let wallpaper_ids = state.collections.resolve_wallpaper_ids(&collection_ids)?;
    let legacy_selection = if matches!(strategy.as_str(), "shuffle" | "weighted_random") {
        "random"
    } else {
        "round_robin"
    };
    let schedule = state.database.configure_rotation(
        &monitor_id,
        &wallpaper_ids,
        interval_seconds,
        &fit_mode,
        legacy_selection,
    )?;
    state.database.set_rotation_policy(
        &monitor_id,
        &strategy,
        &collection_ids,
        &serde_json::to_string(&rules)?,
    )?;
    state.scheduler.wake();
    Ok(schedule)
}

/// Returns the last persisted explanation for one display's selection policy.
#[tauri::command]
pub fn get_rotation_explanation(
    monitor_id: String,
    state: State<'_, AppState>,
) -> AppResult<RotationExplanation> {
    state.database.rotation_explanation(&monitor_id)
}

/// Applies the most recent different wallpaper without changing the configured policy.
#[tauri::command]
pub async fn previous_wallpaper(
    monitor_id: String,
    state: State<'_, AppState>,
) -> AppResult<crate::image_processing::ProcessedImage> {
    let wallpaper = state.database.previous_rotation_wallpaper(&monitor_id)?;
    let fit_mode = state
        .database
        .list_schedules()?
        .into_iter()
        .find(|schedule| schedule.system_monitor_id == monitor_id)
        .map(|schedule| schedule.fit_mode)
        .unwrap_or_else(|| "fill".into());
    let service = wallpaper_service(&state);
    let processed = service
        .apply_to_monitor(
            wallpaper.id,
            &monitor_id,
            crate::image_processing::FitMode::try_from(fit_mode.as_str())?,
            false,
        )
        .await?;
    state
        .database
        .record_manual_history(wallpaper.id, &monitor_id)?;
    state
        .database
        .set_rotation_reason(&monitor_id, "用户选择上一张壁纸")?;
    Ok(processed)
}

/// Skips the current item by consuming the next policy candidate immediately.
#[tauri::command]
pub fn skip_wallpaper(monitor_id: String, state: State<'_, AppState>) -> AppResult<()> {
    state.database.trigger_schedule_now(&monitor_id)?;
    state
        .database
        .set_rotation_reason(&monitor_id, "用户跳过当前壁纸")?;
    state.scheduler.wake();
    Ok(())
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

/// Exposes duplicate file locations without mutating or deleting either copy.
#[tauri::command]
pub fn list_duplicate_file_groups(
    state: State<'_, AppState>,
) -> AppResult<Vec<DuplicateFileGroup>> {
    state.database.list_duplicate_file_groups()
}

/// Lists each built-in provider's independent enablement and health state.
#[tauri::command]
pub fn list_providers(state: State<'_, AppState>) -> AppResult<Vec<ProviderStatus>> {
    state.database.list_provider_status()
}

/// Returns all retained provider attribution for one deduplicated wallpaper.
#[tauri::command]
pub fn list_wallpaper_sources(
    wallpaper_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<WallpaperProviderSource>> {
    state.database.list_wallpaper_sources(wallpaper_id)
}

/// Enables or disables one provider without selecting a global default source.
#[tauri::command]
pub fn update_provider_config(
    provider: String,
    enabled: bool,
    state: State<'_, AppState>,
) -> AppResult<Vec<ProviderStatus>> {
    // Registry resolution prevents arbitrary provider names from entering persisted config.
    state.providers.get(&provider)?;
    state.database.set_provider_enabled(&provider, enabled)?;
    state.database.list_provider_status()
}

/// Lists manual and smart collections in user-defined order.
#[tauri::command]
pub fn list_collections(state: State<'_, AppState>) -> AppResult<Vec<CollectionRecord>> {
    state.collections.list()
}

/// Creates one manual collection without touching wallpaper files.
#[tauri::command]
pub fn create_collection(
    name: String,
    description: String,
    state: State<'_, AppState>,
) -> AppResult<CollectionRecord> {
    state.collections.create(&name, &description)
}

/// Updates collection metadata and optional cover selection.
#[tauri::command]
pub fn update_collection(
    collection_id: i64,
    name: String,
    description: String,
    cover_wallpaper_id: Option<i64>,
    position: i64,
    state: State<'_, AppState>,
) -> AppResult<CollectionRecord> {
    state.collections.update(
        collection_id,
        &name,
        &description,
        cover_wallpaper_id,
        position,
    )
}

/// Deletes only the collection container and membership relationships.
#[tauri::command]
pub fn delete_collection(collection_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    state.collections.delete(collection_id)
}

/// Adds multiple wallpapers to one manual collection in a single transaction.
#[tauri::command]
pub fn add_collection_wallpapers(
    collection_id: i64,
    wallpaper_ids: Vec<i64>,
    state: State<'_, AppState>,
) -> AppResult<usize> {
    state
        .collections
        .add_wallpapers(collection_id, &wallpaper_ids)
}

/// Removes only collection membership for the requested wallpapers.
#[tauri::command]
pub fn remove_collection_wallpapers(
    collection_id: i64,
    wallpaper_ids: Vec<i64>,
    state: State<'_, AppState>,
) -> AppResult<usize> {
    state
        .collections
        .remove_wallpapers(collection_id, &wallpaper_ids)
}

/// Saves one schema-versioned smart rule after previewing its current result set.
#[tauri::command]
pub fn set_smart_collection_rule(
    collection_id: i64,
    rule: SmartCollectionRule,
    state: State<'_, AppState>,
) -> AppResult<WallpaperPage> {
    state.collections.set_smart_rule(collection_id, &rule)
}

/// Previews a smart rule without persisting arbitrary query text.
#[tauri::command]
pub fn preview_smart_collection(
    rule: SmartCollectionRule,
    page: u32,
    page_size: u32,
    state: State<'_, AppState>,
) -> AppResult<WallpaperPage> {
    state.collections.preview_smart_rule(&rule, page, page_size)
}

/// Queries manual or smart collection contents through the bounded page contract.
#[tauri::command]
pub fn query_collection_wallpapers(
    collection_id: i64,
    page: u32,
    page_size: u32,
    state: State<'_, AppState>,
) -> AppResult<WallpaperPage> {
    state.collections.wallpapers(collection_id, page, page_size)
}

/// Fetches one bounded page from all enabled providers without downloading 4K originals.
#[tauri::command]
pub async fn sync_catalog(
    mut query: WallpaperQuery,
    state: State<'_, AppState>,
) -> AppResult<usize> {
    const MAX_PROGRESSIVE_PAGE: u64 = 20;
    let progressive_refresh = query
        .keyword
        .as_deref()
        .is_none_or(|keyword| keyword.trim().is_empty())
        && query.page <= 1;
    if progressive_refresh {
        // Generic refreshes advance through bounded provider pages instead of rewriting page one forever.
        let initial_page = if state
            .database
            .get_setting("resource_sync_last_success")?
            .is_some()
        {
            // Existing installations already synchronized page one before a cursor was introduced.
            2
        } else {
            1
        };
        let stored_page = state
            .database
            .get_setting("resource_sync_next_page")?
            .and_then(|value| value.as_u64())
            .unwrap_or(initial_page)
            .clamp(1, MAX_PROGRESSIVE_PAGE);
        query.page = u32::try_from(stored_page).unwrap_or(1);
    }
    let synchronized_page = query.page;
    let imported = sync_online_metadata(query, &state).await?;
    if progressive_refresh {
        let next_page = if u64::from(synchronized_page) >= MAX_PROGRESSIVE_PAGE {
            1
        } else {
            u64::from(synchronized_page) + 1
        };
        state.database.set_setting(
            "resource_sync_next_page",
            &serde_json::Value::from(next_page),
        )?;
    }
    Ok(imported)
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
    sync_catalog(WallpaperQuery::default(), state).await
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
    // Recreate adapters for every manual/startup sync so long-running apps observe proxy changes.
    let providers = fresh_provider_services(state)?;
    let aggregated_service = AggregatedProviderService::new(state.database.clone(), providers);
    let aggregated = match aggregated_service.search(query.clone()).await {
        Ok(result) => result,
        Err(first_error @ crate::error::AppError::Provider(_)) => {
            tracing::warn!(error = %first_error, "all online providers failed; retrying with fresh network clients");
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
            let retry_providers = fresh_provider_services(state)?;
            AggregatedProviderService::new(state.database.clone(), retry_providers)
                .search(query)
                .await?
        }
        Err(error) => return Err(error),
    };
    for (provider, error) in &aggregated.failures {
        tracing::warn!(
            provider,
            error,
            "provider failed while other sources remained available"
        );
    }
    let synced_at = current_sync_stamp()?;
    let provider_keys = aggregated
        .wallpapers
        .iter()
        .map(|wallpaper| (wallpaper.provider.clone(), wallpaper.remote_id.clone()))
        .collect::<Vec<_>>();
    let records: Vec<_> = aggregated
        .wallpapers
        .into_iter()
        .map(|wallpaper| remote_to_new(wallpaper, &requested_category, &synced_at, None))
        .collect();
    let imported = state.database.upsert_wallpapers(&records)?;
    if let Some(keyword) = requested_keyword.as_deref() {
        // Search provenance stays separate from semantic provider tags and can be replaced safely.
        state
            .database
            .replace_search_results(keyword, &provider_keys)?;
    }
    state.database.set_setting(
        "resource_sync_last_success",
        &serde_json::Value::from(current_unix_seconds()?),
    )?;
    tracing::info!(count = imported, "online wallpaper metadata synchronized");
    Ok(imported)
}

/// Rebuilds provider clients from current settings and the latest operating-system proxy state.
fn fresh_provider_services(state: &State<'_, AppState>) -> AppResult<ProviderServices> {
    let providers = ProviderServices::new(&state.paths)?;
    let api_key = state
        .settings
        .lock()
        .map_err(|_| crate::error::AppError::configuration("settings mutex was poisoned"))?
        .thegamesdb_api_key
        .clone();
    providers.configure_thegamesdb_api_key(api_key.as_deref())?;
    Ok(providers)
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
    let roots = state
        .settings
        .lock()
        .map_err(|_| crate::error::AppError::configuration("settings mutex was poisoned"))?
        .local_directories
        .clone();
    state.database.reconcile_local_file_states(&roots)
}

/// Skips unchanged files by size and mtime, then validates only new or changed image content.
async fn index_local_paths(paths: Vec<PathBuf>, state: &State<'_, AppState>) -> AppResult<usize> {
    let provider = LocalProvider::new(paths);
    let files = provider.discover_files().await?;
    let synced_at = current_sync_stamp()?;
    let mut indexed = 0_usize;
    for local_path in files {
        let metadata = std::fs::metadata(&local_path)?;
        let modified_at_ms = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(|error| crate::error::AppError::FileSystem(error.to_string()))?
            .as_millis();
        let modified_at_ms = u64::try_from(modified_at_ms).map_err(|_| {
            crate::error::AppError::FileSystem("local file timestamp is out of range".into())
        })?;
        if state
            .database
            .local_file_is_unchanged(&local_path, metadata.len(), modified_at_ms)?
        {
            indexed += 1;
            continue;
        }

        let wallpaper = LocalProvider::inspect_file(&local_path)?;
        let thumbnail = state
            .images
            .create_thumbnail(&local_path, Some(640), Some(360))?;
        let record = remote_to_new(
            wallpaper,
            &WallpaperCategory::Local,
            &synced_at,
            Some(thumbnail.path),
        );
        let upserted = state.database.upsert_wallpapers(&[record])?;
        if upserted == 1 {
            state.database.record_local_file_snapshot(
                &local_path,
                metadata.len(),
                modified_at_ms,
            )?;
            indexed += 1;
        }
    }
    Ok(indexed)
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

/// Validates and downscales a user-selected UI background into application-owned cache.
#[tauri::command]
pub fn import_theme_background(
    path: PathBuf,
    state: State<'_, AppState>,
) -> AppResult<ThemeBackgroundData> {
    let processed = state
        .images
        .create_thumbnail(&path, Some(2560), Some(1440))?;
    let retained = state
        .paths
        .config_dir
        .join(format!("theme-background-{}.jpg", processed.source_sha256));
    if !retained.is_file() {
        // Theme backgrounds are preferences, so normal thumbnail cache cleanup must not remove them.
        std::fs::copy(&processed.path, &retained)?;
    }
    load_theme_background_data(&retained, &state)
}

/// Reloads only application-owned background bytes; missing files trigger a frontend fallback.
#[tauri::command]
pub fn load_theme_background(
    path: PathBuf,
    state: State<'_, AppState>,
) -> AppResult<ThemeBackgroundData> {
    load_theme_background_data(&path, &state)
}

/// Enforces the AppData boundary before returning a bounded processed image to WebView.
fn load_theme_background_data(
    path: &std::path::Path,
    state: &AppState,
) -> AppResult<ThemeBackgroundData> {
    let canonical_root = std::fs::canonicalize(&state.paths.root)?;
    let canonical_path = std::fs::canonicalize(path)?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(crate::error::AppError::FileSystem(
            "theme background must be stored inside application data".into(),
        ));
    }
    let metadata = state.images.inspect(&canonical_path)?;
    Ok(ThemeBackgroundData {
        path: canonical_path.display().to_string(),
        mime_type: metadata.mime_type.into(),
        luminance: state.images.average_luminance(&canonical_path)?,
        bytes: std::fs::read(canonical_path)?,
    })
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
    if !matches!(
        settings.theme_pack.as_str(),
        "classic" | "gallery" | "compact" | "glass"
    ) {
        return Err(crate::error::AppError::configuration(
            "unsupported built-in theme pack",
        ));
    }
    if !matches!(
        settings.theme_background_fit.as_str(),
        "fill" | "fit" | "center" | "stretch"
    ) || !(0.0..=0.85).contains(&settings.theme_background_overlay)
    {
        return Err(crate::error::AppError::configuration(
            "invalid application background settings",
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
    // Provider credentials remain optional and whitespace-free in the local config file.
    settings.thegamesdb_api_key = settings
        .thegamesdb_api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
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
    state
        .providers
        .configure_thegamesdb_api_key(settings.thegamesdb_api_key.as_deref())?;
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
        WallpaperCategory::Games => "games".to_owned(),
        WallpaperCategory::People => "people".to_owned(),
        WallpaperCategory::Local => "local".to_owned(),
        WallpaperCategory::All => match wallpaper.category.to_ascii_lowercase().as_str() {
            "anime" => "anime".to_owned(),
            "games" => "games".to_owned(),
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
        perceptual_hash: wallpaper.perceptual_hash,
        download_status: if is_local { "downloaded" } else { "remote" }.into(),
        preset: false,
        created_at: wallpaper.created_at,
        author: wallpaper.author,
        license_name: wallpaper.license_name,
        license_url: wallpaper.license_url,
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

/// Constructs the V2 spanning service while keeping orchestration out of IPC commands.
fn spanning_service<'a>(state: &'a State<'_, AppState>) -> SpannedWallpaperService<'a> {
    SpannedWallpaperService::new(
        &state.database,
        &state.platform,
        &state.providers,
        &state.images,
        &state.paths.wallpapers_original_dir,
    )
}
