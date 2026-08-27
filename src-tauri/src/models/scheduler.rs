use serde::{Deserialize, Serialize};

/// Persisted per-monitor scheduler state restored after application restart.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRecord {
    pub system_monitor_id: String,
    pub enabled: bool,
    pub paused: bool,
    pub interval_seconds: i64,
    pub fit_mode: String,
    pub last_change_at: Option<String>,
    pub next_change_at: String,
    pub last_error: Option<String>,
    pub wallpaper_count: u32,
    pub selection_mode: String,
}
