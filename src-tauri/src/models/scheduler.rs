use serde::{Deserialize, Serialize};

use crate::{
    error::{AppError, AppResult},
    platform::RuntimeEnvironment,
};

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

/// Human-readable persisted explanation for the current per-monitor selection policy.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RotationExplanation {
    pub system_monitor_id: String,
    pub strategy: String,
    pub last_reason: Option<String>,
    pub source_collection_count: u32,
    pub source_collection_ids: Vec<i64>,
    pub candidate_count: u32,
    pub queued_count: u32,
}

/// Versioned declarative constraints; arbitrary code and cron expressions are rejected.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct RotationRules {
    pub version: u32,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub day_group: String,
    pub pause_on_battery: bool,
    pub pause_on_fullscreen: bool,
}

impl Default for RotationRules {
    fn default() -> Self {
        Self {
            version: 1,
            start_time: None,
            end_time: None,
            day_group: "all".into(),
            pause_on_battery: false,
            pause_on_fullscreen: false,
        }
    }
}

impl RotationRules {
    /// Parses and validates the bounded JSON contract before it reaches the worker.
    pub fn from_json(value: &str) -> AppResult<Self> {
        let rules: Self = serde_json::from_str(value)?;
        if rules.version != 1
            || !matches!(rules.day_group.as_str(), "all" | "weekdays" | "weekends")
        {
            return Err(AppError::Configuration(
                "unsupported rotation rule schema".into(),
            ));
        }
        for value in [&rules.start_time, &rules.end_time].into_iter().flatten() {
            parse_minutes(value)?;
        }
        Ok(rules)
    }

    /// Returns a user-facing pause reason or allows exactly one due execution.
    pub fn pause_reason(&self, environment: &RuntimeEnvironment) -> Option<String> {
        let weekend = environment.iso_weekday >= 6;
        if (self.day_group == "weekdays" && weekend) || (self.day_group == "weekends" && !weekend) {
            return Some("当前日期不在配置的工作日/周末范围".into());
        }
        if let (Some(start), Some(end)) = (&self.start_time, &self.end_time) {
            let start = parse_minutes(start).ok()?;
            let end = parse_minutes(end).ok()?;
            let current = environment.local_minutes;
            let allowed = if start <= end {
                current >= start && current <= end
            } else {
                current >= start || current <= end
            };
            if !allowed {
                return Some("当前时间不在允许切换的时间段".into());
            }
        }
        if self.pause_on_battery && environment.on_battery {
            return Some("电池供电时暂停自动切换".into());
        }
        if self.pause_on_fullscreen && environment.fullscreen_app {
            return Some("全屏应用运行时暂停自动切换".into());
        }
        None
    }
}

/// Converts the fixed HH:MM contract without locale or additional native time libraries.
fn parse_minutes(value: &str) -> AppResult<u16> {
    let (hour, minute) = value
        .split_once(':')
        .ok_or_else(|| AppError::Configuration("rotation time must use HH:MM".into()))?;
    let hour = hour
        .parse::<u16>()
        .map_err(|_| AppError::Configuration("rotation hour is invalid".into()))?;
    let minute = minute
        .parse::<u16>()
        .map_err(|_| AppError::Configuration("rotation minute is invalid".into()))?;
    if hour > 23 || minute > 59 {
        return Err(AppError::Configuration(
            "rotation time is out of range".into(),
        ));
    }
    Ok(hour * 60 + minute)
}

#[cfg(test)]
mod tests {
    use super::RotationRules;
    use crate::platform::RuntimeEnvironment;

    #[test]
    fn evaluates_weekdays_overnight_windows_and_runtime_pauses()
    -> Result<(), Box<dyn std::error::Error>> {
        let rules = RotationRules::from_json(
            r#"{"version":1,"startTime":"22:00","endTime":"06:00","dayGroup":"weekdays","pauseOnBattery":true,"pauseOnFullscreen":true}"#,
        )?;
        let monday = RuntimeEnvironment {
            iso_weekday: 1,
            local_minutes: 23 * 60,
            ..RuntimeEnvironment::default()
        };
        assert!(rules.pause_reason(&monday).is_none());
        assert_eq!(
            rules
                .pause_reason(&RuntimeEnvironment {
                    on_battery: true,
                    ..monday
                })
                .as_deref(),
            Some("电池供电时暂停自动切换")
        );
        let saturday = RuntimeEnvironment {
            iso_weekday: 6,
            ..monday
        };
        assert!(rules.pause_reason(&saturday).is_some());
        Ok(())
    }
}
