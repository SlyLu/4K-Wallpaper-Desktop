use std::{path::Path, sync::Arc};

use crate::{error::AppResult, models::MonitorInfo};

/// Platform-neutral signals used only by declarative scheduler pause rules.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeEnvironment {
    pub on_battery: bool,
    pub fullscreen_app: bool,
    pub iso_weekday: u8,
    pub local_minutes: u16,
}

pub trait PlatformEnvironmentService: Send + Sync {
    /// Captures transient power and foreground-window state without persisting app activity.
    fn runtime_environment(&self) -> AppResult<RuntimeEnvironment>;
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// Unified monitor contract consumed by commands and future business services.
pub trait PlatformMonitorService: Send + Sync {
    fn get_monitors(&self) -> AppResult<Vec<MonitorInfo>>;

    /// Derives the primary display from the same native snapshot used by callers.
    fn get_primary_monitor(&self) -> AppResult<MonitorInfo> {
        self.get_monitors()?
            .into_iter()
            .find(|monitor| monitor.primary)
            .ok_or_else(|| crate::error::AppError::monitor("no primary monitor was reported"))
    }
}

/// Unified wallpaper contract that prevents platform APIs from entering business code.
pub trait PlatformWallpaperService: Send + Sync {
    fn set_wallpaper_for_all(&self, image_path: &Path) -> AppResult<()>;
    fn set_wallpaper_for_monitor(&self, monitor_id: &str, image_path: &Path) -> AppResult<()>;

    /// Captures the current native assignment so spanning mode can roll back atomically.
    fn get_wallpaper_for_monitor(&self, monitor_id: &str) -> AppResult<std::path::PathBuf>;

    /// Restores platform-managed assignments after an OS workspace or desktop transition.
    fn reconcile_wallpapers(&self) -> AppResult<usize> {
        Ok(0)
    }
}

/// Active platform adapters shared by Tauri commands.
pub struct PlatformServices {
    pub platform_name: &'static str,
    pub monitors: Arc<dyn PlatformMonitorService>,
    pub wallpaper: Arc<dyn PlatformWallpaperService>,
    pub environment: Arc<dyn PlatformEnvironmentService>,
}

/// Selects one platform implementation at the composition root only.
pub fn create_platform_services() -> AppResult<PlatformServices> {
    #[cfg(target_os = "windows")]
    {
        let adapter = Arc::new(windows::WindowsPlatformAdapter::new());
        return Ok(PlatformServices {
            platform_name: "Windows",
            monitors: adapter.clone(),
            environment: adapter.clone(),
            wallpaper: adapter,
        });
    }

    #[cfg(target_os = "macos")]
    {
        let adapter = Arc::new(macos::MacOsPlatformAdapter::new());
        return Ok(PlatformServices {
            platform_name: "macOS",
            monitors: adapter.clone(),
            environment: adapter.clone(),
            wallpaper: adapter,
        });
    }

    #[allow(unreachable_code)]
    Err(crate::error::AppError::platform(
        "4K Wallpaper Desktop V1 supports only Windows and macOS",
    ))
}

/// Rejects missing files and unsupported formats before calling native desktop APIs.
fn validate_wallpaper_path(image_path: &Path) -> AppResult<std::path::PathBuf> {
    if !image_path.is_file() {
        return Err(crate::error::AppError::Wallpaper(format!(
            "wallpaper file does not exist: {}",
            image_path.display()
        )));
    }
    let extension = image_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "bmp" | "webp") {
        return Err(crate::error::AppError::Wallpaper(format!(
            "unsupported wallpaper format: {extension}"
        )));
    }
    image_path.canonicalize().map_err(Into::into)
}
