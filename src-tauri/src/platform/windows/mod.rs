use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use windows::{
    Win32::{
        Foundation::RECT,
        System::Com::{
            CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
            CoUninitialize,
        },
        UI::Shell::{DesktopWallpaper, IDesktopWallpaper},
    },
    core::{HSTRING, PCWSTR, PWSTR},
};

use crate::{
    error::{AppError, AppResult},
    models::MonitorInfo,
    platform::{PlatformMonitorService, PlatformWallpaperService, validate_wallpaper_path},
};

#[derive(Clone, Default)]
pub struct WindowsPlatformAdapter {
    desired_wallpapers: Arc<Mutex<HashMap<String, PathBuf>>>,
}

impl WindowsPlatformAdapter {
    /// Creates the stateless adapter; each call owns its COM apartment and interface.
    pub fn new() -> Self {
        Self::default()
    }

    /// Remembers only successful native assignments for virtual-desktop reconciliation.
    fn remember_assignment(&self, monitor_id: &str, image_path: &Path) -> AppResult<()> {
        self.desired_wallpapers
            .lock()
            .map_err(|_| AppError::platform("wallpaper assignment mutex was poisoned"))?
            .insert(monitor_id.to_owned(), image_path.to_path_buf());
        Ok(())
    }
}

/// Balances COM initialization for every native call, including early errors.
struct ComApartment;

impl ComApartment {
    fn initialize() -> AppResult<Self> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .map_err(|error| AppError::platform(format!("failed to initialize COM: {error}")))?;
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

impl PlatformMonitorService for WindowsPlatformAdapter {
    /// Enumerates displays through IDesktopWallpaper so returned IDs can set a specific display.
    fn get_monitors(&self) -> AppResult<Vec<MonitorInfo>> {
        let _apartment = ComApartment::initialize()?;
        let wallpaper = create_desktop_wallpaper()?;
        let count = unsafe { wallpaper.GetMonitorDevicePathCount() }
            .map_err(|error| AppError::monitor(format!("failed to count monitors: {error}")))?;
        let mut monitors = Vec::with_capacity(count as usize);
        for index in 0..count {
            let monitor_id = get_monitor_id(&wallpaper, index)?;
            let identifier = HSTRING::from(&monitor_id);
            let bounds = unsafe { wallpaper.GetMonitorRECT(PCWSTR(identifier.as_ptr())) }.map_err(
                |error| AppError::monitor(format!("failed to read monitor bounds: {error}")),
            )?;
            if bounds.right <= bounds.left || bounds.bottom <= bounds.top {
                // IDesktopWallpaper may retain detached displays; V1 exposes only active monitors.
                continue;
            }
            let width = positive_dimension(bounds.right - bounds.left, "width")?;
            let height = positive_dimension(bounds.bottom - bounds.top, "height")?;
            monitors.push(MonitorInfo {
                system_monitor_id: monitor_id,
                name: format!("Display {}", index + 1),
                width,
                height,
                position_x: bounds.left,
                position_y: bounds.top,
                primary: contains_origin(&bounds),
            });
        }
        if monitors.is_empty() {
            return Err(AppError::monitor("Windows reported no active monitors"));
        }
        Ok(monitors)
    }
}

impl PlatformWallpaperService for WindowsPlatformAdapter {
    /// Applies the same image explicitly to every active Windows display.
    fn set_wallpaper_for_all(&self, image_path: &Path) -> AppResult<()> {
        let path = validate_wallpaper_path(image_path)?;
        let monitors = self.get_monitors()?;
        let _apartment = ComApartment::initialize()?;
        let wallpaper = create_desktop_wallpaper()?;
        // Per-monitor calls remain deterministic when displays previously used different images.
        for monitor in &monitors {
            set_native_wallpaper(&wallpaper, Some(&monitor.system_monitor_id), &path)?;
            self.remember_assignment(&monitor.system_monitor_id, &path)?;
        }
        tracing::info!(path = %path.display(), "wallpaper set for all Windows displays");
        Ok(())
    }

    /// Sends the stable monitor device path back to IDesktopWallpaper for per-display assignment.
    fn set_wallpaper_for_monitor(&self, monitor_id: &str, image_path: &Path) -> AppResult<()> {
        let path = validate_wallpaper_path(image_path)?;
        let _apartment = ComApartment::initialize()?;
        let wallpaper = create_desktop_wallpaper()?;
        let available = (0..unsafe { wallpaper.GetMonitorDevicePathCount() }
            .map_err(|error| AppError::monitor(error.to_string()))?)
            .map(|index| get_monitor_id(&wallpaper, index))
            .collect::<AppResult<Vec<_>>>()?;
        if !available.iter().any(|candidate| candidate == monitor_id) {
            return Err(AppError::monitor(format!(
                "monitor is no longer connected: {monitor_id}"
            )));
        }
        set_native_wallpaper(&wallpaper, Some(monitor_id), &path)?;
        self.remember_assignment(monitor_id, &path)?;
        tracing::info!(monitor_id, path = %path.display(), "wallpaper set for Windows display");
        Ok(())
    }

    /// Windows 11 stores wallpaper per virtual desktop; restore the application's last assignments.
    fn reconcile_wallpapers(&self) -> AppResult<usize> {
        let desired = self
            .desired_wallpapers
            .lock()
            .map_err(|_| AppError::platform("wallpaper assignment mutex was poisoned"))?
            .clone();
        if desired.is_empty() {
            return Ok(0);
        }

        let _apartment = ComApartment::initialize()?;
        let wallpaper = create_desktop_wallpaper()?;
        let available = (0..unsafe { wallpaper.GetMonitorDevicePathCount() }
            .map_err(|error| AppError::monitor(error.to_string()))?)
            .map(|index| get_monitor_id(&wallpaper, index))
            .collect::<AppResult<HashSet<_>>>()?;
        let mut restored = 0;
        for (monitor_id, expected_path) in desired {
            if !available.contains(&monitor_id) {
                continue;
            }
            let current_path = get_current_wallpaper(&wallpaper, &monitor_id)?;
            if same_windows_path(&current_path, &expected_path) {
                continue;
            }
            set_native_wallpaper(&wallpaper, Some(&monitor_id), &expected_path)?;
            restored += 1;
            tracing::info!(monitor_id, path = %expected_path.display(), "wallpaper restored after Windows desktop transition");
        }
        Ok(restored)
    }
}

/// Creates the system desktop wallpaper COM interface inside the current apartment.
fn create_desktop_wallpaper() -> AppResult<IDesktopWallpaper> {
    unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL) }
        .map_err(|error| AppError::platform(format!("IDesktopWallpaper is unavailable: {error}")))
}

/// Copies and frees the COM-owned monitor device string returned by Windows.
fn get_monitor_id(wallpaper: &IDesktopWallpaper, index: u32) -> AppResult<String> {
    let pointer: PWSTR = unsafe { wallpaper.GetMonitorDevicePathAt(index) }.map_err(|error| {
        AppError::monitor(format!("failed to read monitor identifier: {error}"))
    })?;
    let value = unsafe { pointer.to_string() }
        .map_err(|error| AppError::monitor(format!("invalid monitor identifier: {error}")))?;
    unsafe { CoTaskMemFree(Some(pointer.as_ptr().cast())) };
    Ok(value)
}

/// Reads and frees the wallpaper path currently assigned to one Windows display.
fn get_current_wallpaper(wallpaper: &IDesktopWallpaper, monitor_id: &str) -> AppResult<PathBuf> {
    let identifier = HSTRING::from(monitor_id);
    let pointer: PWSTR =
        unsafe { wallpaper.GetWallpaper(PCWSTR(identifier.as_ptr())) }.map_err(|error| {
            AppError::Wallpaper(format!("failed to read current wallpaper: {error}"))
        })?;
    let value = unsafe { pointer.to_string() }
        .map_err(|error| AppError::Wallpaper(format!("invalid current wallpaper path: {error}")))?;
    unsafe { CoTaskMemFree(Some(pointer.as_ptr().cast())) };
    if value.is_empty() {
        return Err(AppError::Wallpaper(
            "Windows does not currently have an image wallpaper to restore".into(),
        ));
    }
    Ok(PathBuf::from(value))
}

/// Compares canonical Windows paths case-insensitively and tolerates the extended path prefix.
fn same_windows_path(left: &Path, right: &Path) -> bool {
    fn normalize(path: &Path) -> String {
        let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        resolved
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .replace('/', "\\")
            .to_lowercase()
    }
    normalize(left) == normalize(right)
}

/// Calls IDesktopWallpaper with UTF-16 values whose storage outlives the native call.
fn set_native_wallpaper(
    wallpaper: &IDesktopWallpaper,
    monitor_id: Option<&str>,
    image_path: &Path,
) -> AppResult<()> {
    let path_text = image_path.to_string_lossy();
    let path = HSTRING::from(path_text.as_ref());
    let monitor = monitor_id.map(HSTRING::from);
    let monitor_pointer = monitor
        .as_ref()
        .map_or(PCWSTR::null(), |value| PCWSTR(value.as_ptr()));
    unsafe { wallpaper.SetWallpaper(monitor_pointer, PCWSTR(path.as_ptr())) }
        .map_err(|error| AppError::Wallpaper(format!("Windows rejected the wallpaper: {error}")))
}

/// Treats the Windows virtual-desktop origin as the primary monitor marker.
fn contains_origin(bounds: &RECT) -> bool {
    bounds.left <= 0 && bounds.right > 0 && bounds.top <= 0 && bounds.bottom > 0
}

/// Prevents invalid signed native dimensions from crossing the domain boundary.
fn positive_dimension(value: i32, label: &str) -> AppResult<u32> {
    if value <= 0 {
        return Err(AppError::monitor(format!(
            "monitor {label} is invalid: {value}"
        )));
    }
    u32::try_from(value)
        .map_err(|_| AppError::monitor(format!("monitor {label} is invalid: {value}")))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use windows::Win32::Foundation::RECT;

    use crate::platform::{PlatformMonitorService, PlatformWallpaperService};

    use super::{
        ComApartment, WindowsPlatformAdapter, contains_origin, create_desktop_wallpaper,
        get_current_wallpaper, positive_dimension, same_windows_path, set_native_wallpaper,
    };

    /// Restores the user's original desktop image even when an assertion returns early.
    struct WallpaperRestore {
        wallpaper: windows::Win32::UI::Shell::IDesktopWallpaper,
        monitor_id: String,
        original_path: PathBuf,
    }

    impl Drop for WallpaperRestore {
        fn drop(&mut self) {
            let _ =
                set_native_wallpaper(&self.wallpaper, Some(&self.monitor_id), &self.original_path);
        }
    }

    /// Produces a valid two-pixel 24-bit BMP without introducing an image dependency in Phase 1.
    fn tiny_bmp() -> Vec<u8> {
        vec![
            0x42, 0x4D, 62, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0,
            1, 0, 24, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0xD9, 0xF9, 0xFF, 0x10, 0x28, 0x40, 0, 0,
        ]
    }

    #[test]
    fn identifies_primary_bounds_by_virtual_desktop_origin() {
        assert!(contains_origin(&RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080
        }));
        assert!(!contains_origin(&RECT {
            left: 1920,
            top: 0,
            right: 3840,
            bottom: 1080
        }));
    }

    #[test]
    fn rejects_negative_native_dimensions() {
        assert!(positive_dimension(-1, "width").is_err());
    }

    #[test]
    fn compares_equivalent_windows_wallpaper_paths() {
        assert!(same_windows_path(
            PathBuf::from(r"\\?\C:\Wallpapers\IMAGE.JPG").as_path(),
            PathBuf::from(r"c:\wallpapers\image.jpg").as_path()
        ));
    }

    #[test]
    fn native_monitor_enumeration_reports_a_primary_display()
    -> Result<(), Box<dyn std::error::Error>> {
        let adapter = WindowsPlatformAdapter::new();
        let monitors = adapter.get_monitors()?;
        println!("native monitors: {monitors:#?}");
        assert!(!monitors.is_empty());
        assert_eq!(monitors.iter().filter(|monitor| monitor.primary).count(), 1);
        assert!(
            monitors
                .iter()
                .all(|monitor| monitor.width > 0 && monitor.height > 0)
        );
        Ok(())
    }

    #[test]
    #[ignore = "changes the primary desktop image briefly and restores it"]
    fn native_primary_wallpaper_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        let adapter = WindowsPlatformAdapter::new();
        let primary = adapter.get_primary_monitor()?;
        let _apartment = ComApartment::initialize()?;
        let native = create_desktop_wallpaper()?;
        let original_path = get_current_wallpaper(&native, &primary.system_monitor_id)?;
        let _restore = WallpaperRestore {
            wallpaper: native.clone(),
            monitor_id: primary.system_monitor_id.clone(),
            original_path,
        };

        let directory = tempfile::tempdir()?;
        let test_path = directory.path().join("phase1-native-wallpaper.bmp");
        fs::write(&test_path, tiny_bmp())?;
        adapter.set_wallpaper_for_monitor(&primary.system_monitor_id, &test_path)?;

        let configured_path = get_current_wallpaper(&native, &primary.system_monitor_id)?;
        // Windows strips the extended-length prefix when returning the same path.
        assert_eq!(configured_path.canonicalize()?, test_path.canonicalize()?);
        Ok(())
    }

    #[test]
    #[ignore = "changes every desktop image briefly and restores each display"]
    fn native_all_monitors_wallpaper_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        let adapter = WindowsPlatformAdapter::new();
        let monitors = adapter.get_monitors()?;
        let _apartment = ComApartment::initialize()?;
        let native = create_desktop_wallpaper()?;
        let mut restore_guards = Vec::with_capacity(monitors.len());
        for monitor in &monitors {
            restore_guards.push(WallpaperRestore {
                wallpaper: native.clone(),
                monitor_id: monitor.system_monitor_id.clone(),
                original_path: get_current_wallpaper(&native, &monitor.system_monitor_id)?,
            });
        }

        let directory = tempfile::tempdir()?;
        let test_path = directory.path().join("phase1-all-monitors-wallpaper.bmp");
        fs::write(&test_path, tiny_bmp())?;
        adapter.set_wallpaper_for_all(&test_path)?;

        let expected_path = test_path.canonicalize()?;
        for monitor in &monitors {
            let configured_path = get_current_wallpaper(&native, &monitor.system_monitor_id)?;
            assert_eq!(configured_path.canonicalize()?, expected_path);
        }
        drop(restore_guards);
        Ok(())
    }

    #[test]
    #[ignore = "changes the primary desktop image briefly and validates desktop reconciliation"]
    fn native_virtual_desktop_reconciliation_round_trip() -> Result<(), Box<dyn std::error::Error>>
    {
        let adapter = WindowsPlatformAdapter::new();
        let primary = adapter.get_primary_monitor()?;
        let _apartment = ComApartment::initialize()?;
        let native = create_desktop_wallpaper()?;
        let original_path = get_current_wallpaper(&native, &primary.system_monitor_id)?;
        let _restore = WallpaperRestore {
            wallpaper: native.clone(),
            monitor_id: primary.system_monitor_id.clone(),
            original_path: original_path.clone(),
        };

        let directory = tempfile::tempdir()?;
        let test_path = directory.path().join("virtual-desktop-reconcile.bmp");
        fs::write(&test_path, tiny_bmp())?;
        adapter.set_wallpaper_for_monitor(&primary.system_monitor_id, &test_path)?;
        set_native_wallpaper(&native, Some(&primary.system_monitor_id), &original_path)?;

        assert_eq!(adapter.reconcile_wallpapers()?, 1);
        assert_eq!(
            get_current_wallpaper(&native, &primary.system_monitor_id)?.canonicalize()?,
            test_path.canonicalize()?
        );
        Ok(())
    }
}
