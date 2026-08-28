use std::{
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    error::{AppError, AppResult},
    models::MonitorInfo,
    platform::{
        PlatformEnvironmentService, PlatformMonitorService, PlatformWallpaperService,
        RuntimeEnvironment, validate_wallpaper_path,
    },
};

type CGDirectDisplayId = u32;
type CGError = i32;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGGetActiveDisplayList(
        max_displays: u32,
        active_displays: *mut CGDirectDisplayId,
        display_count: *mut u32,
    ) -> CGError;
    fn CGMainDisplayID() -> CGDirectDisplayId;
    fn CGDisplayPixelsWide(display: CGDirectDisplayId) -> usize;
    fn CGDisplayPixelsHigh(display: CGDirectDisplayId) -> usize;
    fn CGDisplayBounds(display: CGDirectDisplayId) -> CGRect;
}

#[derive(Clone, Default)]
pub struct MacOsPlatformAdapter;

impl MacOsPlatformAdapter {
    /// Creates the stateless CoreGraphics/AppKit adapter.
    pub fn new() -> Self {
        Self
    }
}

impl PlatformMonitorService for MacOsPlatformAdapter {
    /// Enumerates active displays with stable CoreGraphics display IDs and pixel dimensions.
    fn get_monitors(&self) -> AppResult<Vec<MonitorInfo>> {
        const MAX_DISPLAYS: usize = 32;
        let mut identifiers = [0_u32; MAX_DISPLAYS];
        let mut count = 0_u32;
        let result = unsafe {
            CGGetActiveDisplayList(MAX_DISPLAYS as u32, identifiers.as_mut_ptr(), &mut count)
        };
        if result != 0 {
            return Err(AppError::monitor(format!(
                "CoreGraphics display enumeration failed: {result}"
            )));
        }
        let main_display = unsafe { CGMainDisplayID() };
        identifiers[..count as usize]
            .iter()
            .enumerate()
            .map(|(index, identifier)| {
                let bounds = unsafe { CGDisplayBounds(*identifier) };
                Ok(MonitorInfo {
                    system_monitor_id: identifier.to_string(),
                    name: format!("Display {}", index + 1),
                    width: u32::try_from(unsafe { CGDisplayPixelsWide(*identifier) })
                        .map_err(|_| AppError::monitor("display width exceeds V1 model"))?,
                    height: u32::try_from(unsafe { CGDisplayPixelsHigh(*identifier) })
                        .map_err(|_| AppError::monitor("display height exceeds V1 model"))?,
                    position_x: bounds.origin.x.round() as i32,
                    position_y: bounds.origin.y.round() as i32,
                    primary: *identifier == main_display,
                })
            })
            .collect()
    }
}

impl PlatformEnvironmentService for MacOsPlatformAdapter {
    /// Reads battery power and the frontmost window's AXFullScreen attribute through local tools.
    fn runtime_environment(&self) -> AppResult<RuntimeEnvironment> {
        let power = Command::new("pmset").args(["-g", "batt"]).output()?;
        if !power.status.success() {
            return Err(AppError::platform("macOS power status query failed"));
        }
        let on_battery = String::from_utf8_lossy(&power.stdout).contains("Battery Power");
        let script = r#"tell application "System Events"
set frontProcess to first application process whose frontmost is true
try
return value of attribute "AXFullScreen" of window 1 of frontProcess
on error
return false
end try
end tell"#;
        let fullscreen = Command::new("osascript").args(["-e", script]).output()?;
        if !fullscreen.status.success() {
            return Err(AppError::platform("macOS full-screen status query failed"));
        }
        let local_time = Command::new("date").arg("+%u,%H,%M").output()?;
        if !local_time.status.success() {
            return Err(AppError::platform("macOS local time query failed"));
        }
        let local_time = String::from_utf8_lossy(&local_time.stdout);
        let mut values = local_time.trim().split(',');
        let iso_weekday = values
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| AppError::platform("macOS weekday was invalid"))?;
        let hour: u16 = values
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| AppError::platform("macOS hour was invalid"))?;
        let minute: u16 = values
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| AppError::platform("macOS minute was invalid"))?;
        Ok(RuntimeEnvironment {
            on_battery,
            fullscreen_app: String::from_utf8_lossy(&fullscreen.stdout).trim() == "true",
            iso_weekday,
            local_minutes: hour * 60 + minute,
        })
    }
}

impl PlatformWallpaperService for MacOsPlatformAdapter {
    /// Uses AppKit through JavaScript for Automation to update every NSScreen desktop image.
    fn set_wallpaper_for_all(&self, image_path: &Path) -> AppResult<()> {
        let path = validate_wallpaper_path(image_path)?;
        run_appkit_wallpaper_script(None, &path)
    }

    /// Matches NSScreenNumber to the CoreGraphics ID before updating exactly one screen.
    fn set_wallpaper_for_monitor(&self, monitor_id: &str, image_path: &Path) -> AppResult<()> {
        let display_id = monitor_id
            .parse::<u32>()
            .map_err(|_| AppError::monitor(format!("invalid macOS display ID: {monitor_id}")))?;
        let path = validate_wallpaper_path(image_path)?;
        run_appkit_wallpaper_script(Some(display_id), &path)
    }

    /// Reads NSWorkspace's current image URL for rollback without changing desktop state.
    fn get_wallpaper_for_monitor(&self, monitor_id: &str) -> AppResult<PathBuf> {
        let display_id = monitor_id
            .parse::<u32>()
            .map_err(|_| AppError::monitor(format!("invalid macOS display ID: {monitor_id}")))?;
        current_appkit_wallpaper(display_id)
    }
}

/// Resolves one screen's current desktop image through the same AppKit bridge used for writes.
fn current_appkit_wallpaper(display_id: u32) -> AppResult<PathBuf> {
    let script = format!(
        r#"ObjC.import('AppKit');
const target = {display_id};
const workspace = $.NSWorkspace.sharedWorkspace;
for (const screen of $.NSScreen.screens.js) {{
  const number = Number(ObjC.unwrap(screen.deviceDescription.objectForKey('NSScreenNumber')));
  if (number === target) {{
    const url = workspace.desktopImageURLForScreen(screen);
    if (url) console.log(ObjC.unwrap(url.path));
  }}
}}"#
    );
    let output = Command::new("osascript")
        .args(["-l", "JavaScript", "-e", &script])
        .output()?;
    if !output.status.success() {
        return Err(AppError::Wallpaper(format!(
            "macOS wallpaper path query failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if path.is_empty() {
        return Err(AppError::Wallpaper(
            "macOS did not return a desktop image for the requested display".into(),
        ));
    }
    Ok(PathBuf::from(path))
}

/// Runs a small AppKit bridge locally; JSON encoding keeps paths safe inside JavaScript source.
fn run_appkit_wallpaper_script(display_id: Option<u32>, image_path: &Path) -> AppResult<()> {
    let path_json = serde_json::to_string(&image_path.to_string_lossy().as_ref())?;
    let display_expression = display_id.map_or_else(|| "null".into(), |value| value.to_string());
    let script = format!(
        r#"ObjC.import('AppKit'); ObjC.import('Foundation');
const target = {display_expression};
const url = $.NSURL.fileURLWithPath({path_json});
const workspace = $.NSWorkspace.sharedWorkspace;
const screens = $.NSScreen.screens.js;
let changed = 0;
for (const screen of screens) {{
  const number = Number(ObjC.unwrap(screen.deviceDescription.objectForKey('NSScreenNumber')));
  if (target === null || number === target) {{
    const error = Ref();
    if (!workspace.setDesktopImageURLForScreenOptionsError(url, screen, {{}}, error)) {{
      throw new Error('NSWorkspace rejected desktop image');
    }}
    changed += 1;
  }}
}}
if (changed === 0) throw new Error('requested display is not connected');"#
    );
    let output = Command::new("osascript")
        .args(["-l", "JavaScript", "-e", &script])
        .output()?;
    if !output.status.success() {
        return Err(AppError::Wallpaper(format!(
            "macOS rejected the wallpaper: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    tracing::info!(display_id, path = %image_path.display(), "wallpaper set through macOS AppKit");
    Ok(())
}
