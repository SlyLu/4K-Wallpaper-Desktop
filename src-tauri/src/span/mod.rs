use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    error::{AppError, AppResult},
    models::MonitorInfo,
};

#[cfg(not(test))]
mod service;
#[cfg(not(test))]
pub use service::{SpannedWallpaperService, SpanningApplyResult};

/// Geometry needed to render one monitor from a shared virtual desktop canvas.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSlice {
    pub system_monitor_id: String,
    pub canvas_x: u32,
    pub canvas_y: u32,
    pub width: u32,
    pub height: u32,
}

/// Normalized virtual-desktop bounds and deterministic per-monitor slices.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorLayout {
    pub layout_hash: String,
    pub origin_x: i32,
    pub origin_y: i32,
    pub width: u32,
    pub height: u32,
    pub slices: Vec<MonitorSlice>,
}

/// Native assignment captured before spanning mode, used for explicit rollback.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviousWallpaper {
    pub system_monitor_id: String,
    pub path: String,
}

/// Converts signed operating-system monitor coordinates into a zero-based render canvas.
pub fn calculate_monitor_layout(monitors: &[MonitorInfo]) -> AppResult<MonitorLayout> {
    if monitors.is_empty() {
        return Err(AppError::Monitor(
            "cannot build a spanning layout without monitors".into(),
        ));
    }

    let min_x = monitors
        .iter()
        .map(|monitor| i64::from(monitor.position_x))
        .min()
        .ok_or_else(|| AppError::Monitor("monitor layout has no horizontal origin".into()))?;
    let min_y = monitors
        .iter()
        .map(|monitor| i64::from(monitor.position_y))
        .min()
        .ok_or_else(|| AppError::Monitor("monitor layout has no vertical origin".into()))?;
    let max_x = monitors
        .iter()
        .map(|monitor| i64::from(monitor.position_x) + i64::from(monitor.width))
        .max()
        .ok_or_else(|| AppError::Monitor("monitor layout has no horizontal extent".into()))?;
    let max_y = monitors
        .iter()
        .map(|monitor| i64::from(monitor.position_y) + i64::from(monitor.height))
        .max()
        .ok_or_else(|| AppError::Monitor("monitor layout has no vertical extent".into()))?;

    let width = u32::try_from(max_x - min_x)
        .map_err(|_| AppError::Monitor("virtual desktop width is out of range".into()))?;
    let height = u32::try_from(max_y - min_y)
        .map_err(|_| AppError::Monitor("virtual desktop height is out of range".into()))?;
    if width == 0 || height == 0 {
        return Err(AppError::Monitor(
            "virtual desktop dimensions must be positive".into(),
        ));
    }

    let mut slices = monitors
        .iter()
        .map(|monitor| {
            if monitor.width == 0 || monitor.height == 0 {
                return Err(AppError::Monitor(format!(
                    "monitor {} has invalid dimensions",
                    monitor.system_monitor_id
                )));
            }
            Ok(MonitorSlice {
                system_monitor_id: monitor.system_monitor_id.clone(),
                canvas_x: u32::try_from(i64::from(monitor.position_x) - min_x)
                    .map_err(|_| AppError::Monitor("monitor x offset is out of range".into()))?,
                canvas_y: u32::try_from(i64::from(monitor.position_y) - min_y)
                    .map_err(|_| AppError::Monitor("monitor y offset is out of range".into()))?,
                width: monitor.width,
                height: monitor.height,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    // Stable ordering makes snapshots comparable even when native enumeration order changes.
    slices.sort_by(|left, right| {
        (left.canvas_y, left.canvas_x, &left.system_monitor_id).cmp(&(
            right.canvas_y,
            right.canvas_x,
            &right.system_monitor_id,
        ))
    });

    let layout_hash = calculate_layout_hash(min_x, min_y, width, height, &slices);
    Ok(MonitorLayout {
        layout_hash,
        origin_x: i32::try_from(min_x)
            .map_err(|_| AppError::Monitor("virtual desktop x origin is out of range".into()))?,
        origin_y: i32::try_from(min_y)
            .map_err(|_| AppError::Monitor("virtual desktop y origin is out of range".into()))?,
        width,
        height,
        slices,
    })
}

/// Produces a stable snapshot identifier from geometry and monitor identity only.
fn calculate_layout_hash(
    origin_x: i64,
    origin_y: i64,
    width: u32,
    height: u32,
    slices: &[MonitorSlice],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(origin_x.to_le_bytes());
    hasher.update(origin_y.to_le_bytes());
    hasher.update(width.to_le_bytes());
    hasher.update(height.to_le_bytes());
    for slice in slices {
        hasher.update(slice.system_monitor_id.as_bytes());
        hasher.update(slice.canvas_x.to_le_bytes());
        hasher.update(slice.canvas_y.to_le_bytes());
        hasher.update(slice.width.to_le_bytes());
        hasher.update(slice.height.to_le_bytes());
    }
    format!("{:x}", hasher.finalize())[..16].to_owned()
}

#[cfg(test)]
mod tests {
    use super::calculate_monitor_layout;
    use crate::models::MonitorInfo;

    /// Builds one monitor fixture while keeping the layout assertions readable.
    fn monitor(id: &str, width: u32, height: u32, x: i32, y: i32) -> MonitorInfo {
        MonitorInfo {
            system_monitor_id: id.into(),
            name: id.into(),
            width,
            height,
            position_x: x,
            position_y: y,
            primary: x == 0 && y == 0,
        }
    }

    #[test]
    fn normalizes_negative_and_vertical_monitor_offsets() -> Result<(), Box<dyn std::error::Error>>
    {
        let layout = calculate_monitor_layout(&[
            monitor("primary", 2560, 1600, 0, 0),
            monitor("upper-left", 1920, 1080, -1920, -1080),
        ])?;

        assert_eq!((layout.origin_x, layout.origin_y), (-1920, -1080));
        assert_eq!((layout.width, layout.height), (4480, 2680));
        assert_eq!(
            (layout.slices[0].canvas_x, layout.slices[0].canvas_y),
            (0, 0)
        );
        assert_eq!(
            (layout.slices[1].canvas_x, layout.slices[1].canvas_y),
            (1920, 1080)
        );
        Ok(())
    }

    #[test]
    fn rejects_empty_or_zero_sized_layouts() {
        assert!(calculate_monitor_layout(&[]).is_err());
        assert!(calculate_monitor_layout(&[monitor("invalid", 0, 1080, 0, 0)]).is_err());
    }
}
