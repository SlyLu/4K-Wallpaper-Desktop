#![cfg_attr(test, allow(dead_code))]

mod cache;
mod collection;
#[cfg(not(test))]
mod commands;
mod db;
#[cfg(not(test))]
mod desktop;
mod error;
mod image_processing;
mod models;
mod paths;
mod platform;
mod preset;
mod provider;
#[cfg(not(test))]
mod scheduler;
mod settings;
mod span;
mod wallpaper;

const AUTOSTART_ARGUMENT: &str = "--hidden";

/// Distinguishes operating-system auto-start from a user-initiated launch.
fn is_background_launch(args: &[std::ffi::OsString]) -> bool {
    args.iter().any(|argument| argument == AUTOSTART_ARGUMENT)
}

#[cfg(not(test))]
use cache::CacheService;
#[cfg(not(test))]
use collection::CollectionService;
#[cfg(not(test))]
use db::Database;
#[cfg(not(test))]
use error::{AppError, AppResult};
#[cfg(not(test))]
use image_processing::ImageProcessor;
#[cfg(not(test))]
use paths::AppPaths;
#[cfg(not(test))]
use platform::PlatformServices;
#[cfg(not(test))]
use provider::ProviderServices;
#[cfg(not(test))]
use scheduler::SchedulerService;
#[cfg(not(test))]
use settings::AppConfig;
#[cfg(not(test))]
use std::sync::Mutex;
#[cfg(not(test))]
use tauri::{Manager, RunEvent};
#[cfg(not(test))]
use tauri_plugin_autostart::ManagerExt;

/// Shared application resources initialized once during Tauri setup.
#[cfg(not(test))]
pub struct AppState {
    pub database: Database,
    pub paths: AppPaths,
    pub platform: PlatformServices,
    pub providers: ProviderServices,
    pub images: ImageProcessor,
    pub scheduler: SchedulerService,
    pub collections: CollectionService,
    pub settings: Mutex<AppConfig>,
}

#[cfg(not(test))]
impl AppState {
    /// Groups initialized services behind one Tauri-managed application state.
    fn new(
        database: Database,
        paths: AppPaths,
        platform: PlatformServices,
        providers: ProviderServices,
        images: ImageProcessor,
        scheduler: SchedulerService,
        collections: CollectionService,
        settings: AppConfig,
    ) -> Self {
        Self {
            database,
            paths,
            platform,
            providers,
            images,
            scheduler,
            collections,
            settings: Mutex::new(settings),
        }
    }
}

/// Builds and runs the Tauri application with all Phase 0/1 services initialized.
#[cfg(not(test))]
pub fn run() -> AppResult<()> {
    let launch_args = std::env::args_os().collect::<Vec<_>>();
    let background_launch = is_background_launch(&launch_args);
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_ARGUMENT]),
        ))
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let close_to_tray = window
                    .state::<AppState>()
                    .settings
                    .lock()
                    .map(|settings| settings.close_to_tray)
                    .unwrap_or(true);
                if close_to_tray {
                    api.prevent_close();
                    if let Err(error) = window.hide() {
                        tracing::warn!(%error, "main window could not hide to tray");
                    }
                }
            }
        })
        .setup(move |app| {
            let paths = AppPaths::discover()?;
            paths.ensure_directories()?;
            logging::initialize(&paths.logs_dir)?;

            tracing::info!(app_data = %paths.root.display(), "application starting");
            let config = AppConfig::load_or_create(&paths.config_file)?;
            tracing::info!(provider = %config.online_provider, "configuration loaded");
            let autolaunch = app.autolaunch();
            let autostart_result = if config.auto_start {
                autolaunch.enable()
            } else {
                autolaunch.disable()
            };
            if let Err(error) = autostart_result {
                // A missing disabled registration and transient platform failures must not block startup.
                tracing::warn!(%error, enabled = config.auto_start, "auto start synchronization was skipped");
            }

            let database = Database::open(&paths.database_file)?;
            let preset_root = app
                .path()
                .resolve("resources/presets", tauri::path::BaseDirectory::Resource)
                .map_err(|error| AppError::configuration(error.to_string()))?;
            let imported =
                preset::import_bundled_presets(&database, &preset_root, &paths.thumbnails_dir)?;
            tracing::info!(count = imported, "bundled preset catalog imported");
            let platform = platform::create_platform_services()?;
            let providers = ProviderServices::new(&paths)?;
            providers.configure_thegamesdb_api_key(config.thegamesdb_api_key.as_deref())?;
            let images =
                ImageProcessor::new(paths.thumbnails_dir.clone(), paths.processed_dir.clone());
            let scheduler = SchedulerService::new();
            let collections = CollectionService::new(database.clone());
            app.manage(AppState::new(
                database,
                paths,
                platform,
                providers,
                images,
                scheduler.clone(),
                collections,
                config,
            ));
            let state = app.state::<AppState>();
            let cache_limit = state
                .settings
                .lock()
                .map_err(|_| AppError::configuration("settings mutex was poisoned"))?
                .cache_limit_bytes;
            let cleanup = CacheService::new(state.database.clone(), state.paths.clone())
                .enforce_limit(cache_limit)?;
            tracing::info!(
                freed_bytes = cleanup.freed_bytes,
                "startup cache limit checked"
            );
            desktop::setup_tray(app)?;
            if background_launch {
                tracing::info!("auto-start launch is staying hidden in the system tray");
            } else {
                desktop::show_main_window(app.handle());
            }
            scheduler.start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_status,
            commands::get_monitors,
            commands::get_monitor_layout,
            commands::set_wallpaper,
            commands::set_wallpaper_for_monitor,
            commands::list_wallpapers,
            commands::get_wallpaper_thumbnail,
            commands::refresh_wallpaper_thumbnail,
            commands::inspect_image_file,
            commands::create_thumbnail,
            commands::prepare_wallpaper_for_monitor,
            commands::provider_latest,
            commands::provider_search,
            commands::provider_detail,
            commands::provider_download,
            commands::download_wallpaper,
            commands::get_wallpaper_original_bytes,
            commands::apply_catalog_wallpaper,
            commands::apply_spanning_wallpaper,
            commands::disable_spanning_wallpaper,
            commands::set_wallpaper_favorite,
            commands::set_wallpaper_blacklisted,
            commands::delete_wallpaper_cache,
            commands::configure_wallpaper_rotation,
            commands::get_rotation_selection,
            commands::set_rotation_selection,
            commands::configure_rotation_policy,
            commands::get_rotation_explanation,
            commands::get_rotation_rules,
            commands::previous_wallpaper,
            commands::skip_wallpaper,
            commands::get_scheduler_status,
            commands::pause_scheduler,
            commands::resume_scheduler,
            commands::trigger_next_wallpaper,
            commands::query_catalog,
            commands::list_duplicate_file_groups,
            commands::list_providers,
            commands::list_wallpaper_sources,
            commands::update_provider_config,
            commands::list_collections,
            commands::create_collection,
            commands::update_collection,
            commands::delete_collection,
            commands::add_collection_wallpapers,
            commands::remove_collection_wallpapers,
            commands::set_smart_collection_rule,
            commands::preview_smart_collection,
            commands::query_collection_wallpapers,
            commands::sync_catalog,
            commands::sync_catalog_if_due,
            commands::scan_local_directory,
            commands::import_local_paths,
            commands::remove_local_wallpaper,
            commands::prune_missing_local_wallpapers,
            commands::remove_local_directory,
            commands::get_settings,
            commands::import_theme_background,
            commands::load_theme_background,
            commands::update_settings,
            commands::get_cache_info,
            commands::clear_cache,
        ])
        .build(tauri::generate_context!())
        .map_err(|error| AppError::unknown(error.to_string()))?;

    app.run(|_handle, event| {
        if matches!(event, RunEvent::Exit | RunEvent::ExitRequested { .. }) {
            tracing::info!("application exiting");
        }
    });
    Ok(())
}

#[cfg(test)]
mod startup_tests {
    use std::ffi::OsString;

    use super::is_background_launch;

    #[test]
    fn recognizes_only_the_explicit_background_launch_argument() {
        assert!(is_background_launch(&[
            OsString::from("wallpaper-desktop.exe"),
            OsString::from("--hidden"),
        ]));
        assert!(!is_background_launch(&[
            OsString::from("wallpaper-desktop.exe"),
            OsString::from("--hidden-window"),
        ]));
        assert!(!is_background_launch(&[OsString::from(
            "wallpaper-desktop.exe"
        )]));
    }
}

#[cfg(not(test))]
mod logging {
    use std::{fs::OpenOptions, path::Path, sync::Mutex};

    use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

    use crate::error::{AppError, AppResult};

    /// Installs synchronous file logging so crash-adjacent records are not buffered away.
    pub fn initialize(log_directory: &Path) -> AppResult<()> {
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_directory.join("wallpaper-desktop.log"))?;
        // Product-required lifecycle records stay enabled even when the parent shell sets RUST_LOG.
        let filter = EnvFilter::new("info,wallpaper_desktop_lib=debug");

        tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(Mutex::new(log_file)),
            )
            .try_init()
            .map_err(|error| {
                AppError::configuration(format!("failed to initialize logging: {error}"))
            })?;
        Ok(())
    }
}
