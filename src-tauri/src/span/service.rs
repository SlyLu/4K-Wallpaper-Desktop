use std::{path::Path, path::PathBuf};

use serde::Serialize;

use crate::{
    db::Database,
    error::{AppError, AppResult},
    image_processing::{ImageProcessor, SpanFitMode, SpanningSliceImage},
    models::WallpaperRecord,
    platform::PlatformServices,
    provider::ProviderServices,
    wallpaper::WallpaperService,
};

use super::{MonitorLayout, PreviousWallpaper, calculate_monitor_layout};

/// Applied snapshot exposes geometry and generated files for diagnostics and UI feedback.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpanningApplyResult {
    pub layout: MonitorLayout,
    pub slices: Vec<SpanningSliceImage>,
    pub wallpaper: WallpaperRecord,
}

/// V2 orchestration boundary for virtual-canvas rendering, native apply, and rollback.
pub struct SpannedWallpaperService<'a> {
    database: &'a Database,
    platform: &'a PlatformServices,
    providers: &'a ProviderServices,
    images: &'a ImageProcessor,
    original_directory: &'a Path,
}

impl<'a> SpannedWallpaperService<'a> {
    pub fn new(
        database: &'a Database,
        platform: &'a PlatformServices,
        providers: &'a ProviderServices,
        images: &'a ImageProcessor,
        original_directory: &'a Path,
    ) -> Self {
        Self {
            database,
            platform,
            providers,
            images,
            original_directory,
        }
    }

    /// Generates and applies a coherent span, rolling every screen back on partial failure.
    pub async fn apply(
        &self,
        wallpaper_id: i64,
        fit_mode: SpanFitMode,
    ) -> AppResult<SpanningApplyResult> {
        let wallpaper = WallpaperService::new(
            self.database,
            self.providers,
            self.images,
            self.platform,
            self.original_directory,
        )
        .ensure_original(wallpaper_id)
        .await?;
        let monitors = self.platform.monitors.get_monitors()?;
        if monitors.len() < 2 {
            return Err(AppError::Monitor(
                "spanning mode requires at least two active displays".into(),
            ));
        }
        let layout = calculate_monitor_layout(&monitors)?;
        let source = wallpaper
            .local_path
            .as_deref()
            .ok_or_else(|| AppError::Wallpaper("spanning source is unavailable".into()))?;
        let processor = self.images.clone();
        let source = PathBuf::from(source);
        let render_layout = layout.clone();
        let slices = tokio::task::spawn_blocking(move || {
            processor.prepare_spanning_slices(&source, &render_layout, fit_mode)
        })
        .await
        .map_err(|error| AppError::Image(format!("span render task failed: {error}")))??;
        let mut previous = self.database.active_spanning_previous()?;
        if previous.is_empty() {
            previous = monitors
                .iter()
                .map(|monitor| {
                    self.platform
                        .wallpaper
                        .get_wallpaper_for_monitor(&monitor.system_monitor_id)
                        .map(|path| PreviousWallpaper {
                            system_monitor_id: monitor.system_monitor_id.clone(),
                            path: path.display().to_string(),
                        })
                })
                .collect::<AppResult<Vec<_>>>()?;
        }
        for slice in &slices {
            if let Err(error) = self
                .platform
                .wallpaper
                .set_wallpaper_for_monitor(&slice.system_monitor_id, Path::new(&slice.path))
            {
                let rollback_errors = self.restore_previous(&previous);
                let message = format!(
                    "display {} rejected its span slice: {}{}",
                    slice.system_monitor_id, error, rollback_errors
                );
                self.database.save_spanning_failure(
                    wallpaper.id,
                    fit_mode.slug(),
                    &previous,
                    &message,
                )?;
                return Err(AppError::Wallpaper(message));
            }
        }
        self.database.save_spanning_assignment(
            wallpaper.id,
            &layout,
            fit_mode.slug(),
            &previous,
        )?;
        Ok(SpanningApplyResult {
            layout,
            slices,
            wallpaper,
        })
    }

    /// Restores the independent per-monitor assignments captured before span activation.
    pub fn disable(&self) -> AppResult<usize> {
        let previous = self.database.active_spanning_previous()?;
        if previous.is_empty() {
            return Ok(0);
        }
        let rollback_errors = self.restore_previous(&previous);
        if !rollback_errors.is_empty() {
            self.database.deactivate_spanning(Some(&rollback_errors))?;
            return Err(AppError::Wallpaper(rollback_errors));
        }
        self.database.deactivate_spanning(None)?;
        Ok(previous.len())
    }

    /// Attempts every restore so a detached display does not block connected screens.
    fn restore_previous(&self, previous: &[PreviousWallpaper]) -> String {
        let failures = previous
            .iter()
            .filter_map(|assignment| {
                self.platform
                    .wallpaper
                    .set_wallpaper_for_monitor(
                        &assignment.system_monitor_id,
                        Path::new(&assignment.path),
                    )
                    .err()
                    .map(|error| format!("{}: {}", assignment.system_monitor_id, error))
            })
            .collect::<Vec<_>>();
        if failures.is_empty() {
            String::new()
        } else {
            format!("; rollback failures: {}", failures.join(" | "))
        }
    }
}
