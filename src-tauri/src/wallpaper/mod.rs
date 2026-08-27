use std::path::{Path, PathBuf};

use crate::{
    db::Database,
    error::{AppError, AppResult},
    image_processing::{FitMode, ImageMetadata, ImageProcessor, ProcessedImage},
    models::WallpaperRecord,
    platform::PlatformServices,
    provider::{ProviderServices, RemoteWallpaper},
};

/// Phase 5 orchestration boundary for download, deduplication, processing, setting, and history.
pub struct WallpaperService<'a> {
    database: &'a Database,
    providers: &'a ProviderServices,
    images: &'a ImageProcessor,
    platform: &'a PlatformServices,
    original_directory: &'a Path,
}

impl<'a> WallpaperService<'a> {
    /// Borrows long-lived application services without duplicating platform or provider state.
    pub fn new(
        database: &'a Database,
        providers: &'a ProviderServices,
        images: &'a ImageProcessor,
        platform: &'a PlatformServices,
        original_directory: &'a Path,
    ) -> Self {
        Self {
            database,
            providers,
            images,
            platform,
            original_directory,
        }
    }

    /// Returns a verified local original, downloading and content-deduplicating only when needed.
    pub async fn ensure_original(&self, wallpaper_id: i64) -> AppResult<WallpaperRecord> {
        let wallpaper = self.database.get_wallpaper(wallpaper_id)?;
        if wallpaper.blacklisted {
            return Err(AppError::Wallpaper(
                "blacklisted wallpaper cannot be downloaded or applied".into(),
            ));
        }
        if let Some(path) = wallpaper.local_path.as_deref() {
            let path = PathBuf::from(path);
            if path.is_file() {
                let metadata = self.inspect_on_worker(path.clone()).await?;
                return self
                    .database
                    .mark_wallpaper_downloaded(wallpaper_id, &path, &metadata);
            }
        }

        let remote = record_to_remote(&wallpaper);
        let provider = self.providers.get(&wallpaper.provider)?;
        let downloaded = provider.download(&remote).await?;
        let metadata = self.inspect_on_worker(downloaded.clone()).await?;
        let retained_path = if let Some(existing) = self
            .database
            .downloaded_path_by_hash(&metadata.sha256, wallpaper_id)?
            .filter(|path| Path::new(path).is_file())
        {
            let existing = PathBuf::from(existing);
            self.remove_duplicate_download(&downloaded, &existing)?;
            existing
        } else {
            downloaded
        };
        self.database
            .mark_wallpaper_downloaded(wallpaper_id, &retained_path, &metadata)
    }

    /// Processes and applies one catalog item to a concrete active monitor.
    pub async fn apply_to_monitor(
        &self,
        wallpaper_id: i64,
        system_monitor_id: &str,
        fit_mode: FitMode,
        record_manual_history: bool,
    ) -> AppResult<ProcessedImage> {
        let wallpaper = self.ensure_original(wallpaper_id).await?;
        let original = wallpaper
            .local_path
            .as_deref()
            .ok_or_else(|| AppError::Wallpaper("downloaded wallpaper has no local path".into()))?;
        let monitors = self.platform.monitors.get_monitors()?;
        self.database.upsert_monitors(&monitors)?;
        let monitor = monitors
            .into_iter()
            .find(|monitor| monitor.system_monitor_id == system_monitor_id)
            .ok_or_else(|| AppError::Monitor("selected monitor is not active".into()))?;
        let processor = self.images.clone();
        let original = PathBuf::from(original);
        let processed = tokio::task::spawn_blocking(move || {
            processor.prepare_for_display(&original, monitor.width, monitor.height, fit_mode)
        })
        .await
        .map_err(|error| AppError::Image(format!("image task failed: {error}")))??;
        self.platform
            .wallpaper
            .set_wallpaper_for_monitor(system_monitor_id, Path::new(&processed.path))?;
        if record_manual_history {
            self.database
                .record_manual_history(wallpaper_id, system_monitor_id)?;
        }
        Ok(processed)
    }

    /// Updates catalog preference state through the Phase 5 database boundary.
    pub fn set_favorite(&self, wallpaper_id: i64, favorite: bool) -> AppResult<WallpaperRecord> {
        self.database.set_wallpaper_favorite(wallpaper_id, favorite)
    }

    /// Blacklisting removes the item from rotation pools before returning refreshed metadata.
    pub fn set_blacklisted(
        &self,
        wallpaper_id: i64,
        blacklisted: bool,
    ) -> AppResult<WallpaperRecord> {
        self.database
            .set_wallpaper_blacklisted(wallpaper_id, blacklisted)
    }

    /// Decodes on the blocking pool so 4K validation never stalls the async command runtime.
    async fn inspect_on_worker(&self, path: PathBuf) -> AppResult<ImageMetadata> {
        let processor = self.images.clone();
        tokio::task::spawn_blocking(move || processor.inspect(&path))
            .await
            .map_err(|error| AppError::Image(format!("image task failed: {error}")))?
    }

    /// Deletes only a newly downloaded duplicate inside the application-owned original cache.
    fn remove_duplicate_download(&self, downloaded: &Path, retained: &Path) -> AppResult<()> {
        let downloaded = downloaded.canonicalize()?;
        let retained = retained.canonicalize()?;
        let original_root = self.original_directory.canonicalize()?;
        if downloaded != retained && downloaded.starts_with(original_root) {
            std::fs::remove_file(&downloaded)?;
            tracing::info!(duplicate = %downloaded.display(), retained = %retained.display(), "duplicate original removed");
        }
        Ok(())
    }
}

/// Converts persisted metadata back into the provider-neutral download contract.
fn record_to_remote(wallpaper: &WallpaperRecord) -> RemoteWallpaper {
    RemoteWallpaper {
        remote_id: wallpaper.remote_id.clone(),
        provider: wallpaper.provider.clone(),
        name: wallpaper.name.clone(),
        source_page_url: wallpaper.source_page_url.clone(),
        original_url: wallpaper.original_url.clone(),
        thumbnail_url: wallpaper.thumbnail_url.clone(),
        local_path: wallpaper.local_path.as_ref().map(PathBuf::from),
        width: Some(wallpaper.width),
        height: Some(wallpaper.height),
        resolution: Some(format!("{}x{}", wallpaper.width, wallpaper.height)),
        ratio: wallpaper.aspect_ratio.clone(),
        file_size: wallpaper.file_size,
        mime_type: wallpaper.mime_type.clone(),
        category: wallpaper.category.clone(),
        purity: wallpaper.purity.clone(),
        tags: wallpaper.tags.clone(),
        created_at: wallpaper.created_at.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        sync::{Arc, Mutex},
    };

    use image::{Rgb, RgbImage};

    use super::WallpaperService;
    use crate::{
        db::Database,
        image_processing::{FitMode, ImageProcessor},
        models::{MonitorInfo, NewWallpaper},
        paths::AppPaths,
        platform::{PlatformMonitorService, PlatformServices, PlatformWallpaperService},
        provider::ProviderServices,
    };

    struct MockMonitors;

    impl PlatformMonitorService for MockMonitors {
        fn get_monitors(&self) -> crate::error::AppResult<Vec<MonitorInfo>> {
            Ok(vec![MonitorInfo {
                system_monitor_id: "TEST-DISPLAY".into(),
                name: "Test Display".into(),
                width: 100,
                height: 60,
                position_x: 0,
                position_y: 0,
                primary: true,
            }])
        }
    }

    struct MockWallpaper {
        applied: Arc<Mutex<Vec<String>>>,
    }

    impl PlatformWallpaperService for MockWallpaper {
        fn set_wallpaper_for_all(&self, image_path: &Path) -> crate::error::AppResult<()> {
            self.applied
                .lock()
                .map_err(|_| crate::error::AppError::Platform("mock mutex poisoned".into()))?
                .push(image_path.display().to_string());
            Ok(())
        }

        fn set_wallpaper_for_monitor(
            &self,
            _monitor_id: &str,
            image_path: &Path,
        ) -> crate::error::AppResult<()> {
            self.set_wallpaper_for_all(image_path)
        }
    }

    /// Builds application-owned paths rooted entirely inside one test directory.
    fn test_paths(root: &Path) -> AppPaths {
        AppPaths {
            root: root.to_path_buf(),
            data_dir: root.join("data"),
            database_file: root.join("data/test.db"),
            wallpapers_original_dir: root.join("wallpapers/original"),
            thumbnails_dir: root.join("cache/thumbnails"),
            processed_dir: root.join("cache/processed"),
            logs_dir: root.join("logs"),
            config_dir: root.join("config"),
            config_file: root.join("config/settings.json"),
        }
    }

    #[tokio::test]
    async fn applies_local_catalog_item_and_records_manual_history()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let paths = test_paths(directory.path());
        paths.ensure_directories()?;
        let source = directory.path().join("source.png");
        RgbImage::from_pixel(160, 90, Rgb([20, 80, 140])).save(&source)?;
        let database = Database::open(&paths.database_file)?;
        database.upsert_wallpapers(&[NewWallpaper {
            provider: "local".into(),
            remote_id: "local-test".into(),
            name: "Local Test".into(),
            source_page_url: None,
            original_url: None,
            thumbnail_url: None,
            thumbnail_local_path: None,
            local_path: Some(source.display().to_string()),
            width: 160,
            height: 90,
            aspect_ratio: Some("16:9".into()),
            file_size: None,
            mime_type: Some("image/png".into()),
            category: "local".into(),
            purity: "local".into(),
            hash: None,
            download_status: "downloaded".into(),
            preset: false,
            created_at: None,
            synced_at: "2026-08-24T00:00:00Z".into(),
            tags: Vec::new(),
        }])?;
        let wallpaper_id = database.list_wallpapers(1, 10, false)?.items[0].id;
        let applied = Arc::new(Mutex::new(Vec::new()));
        let platform = PlatformServices {
            platform_name: "test",
            monitors: Arc::new(MockMonitors),
            wallpaper: Arc::new(MockWallpaper {
                applied: Arc::clone(&applied),
            }),
        };
        let providers = ProviderServices::new(&paths)?;
        let images = ImageProcessor::new(paths.thumbnails_dir, paths.processed_dir);
        let service = WallpaperService::new(
            &database,
            &providers,
            &images,
            &platform,
            &paths.wallpapers_original_dir,
        );
        let processed = service
            .apply_to_monitor(wallpaper_id, "TEST-DISPLAY", FitMode::Fill, true)
            .await?;
        assert_eq!((processed.width, processed.height), (100, 60));
        assert_eq!(applied.lock().map_err(|_| "mock mutex poisoned")?.len(), 1);
        assert!(database.get_wallpaper(wallpaper_id)?.last_used_at.is_some());
        Ok(())
    }
}
