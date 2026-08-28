use std::{sync::Arc, time::Duration};

use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

use crate::{
    AppState, cache::CacheService, error::AppResult, image_processing::FitMode,
    models::ScheduleRecord, wallpaper::WallpaperService,
};

const SCHEDULER_POLL_INTERVAL: Duration = Duration::from_secs(15);
const WALLPAPER_RECONCILE_INTERVAL: Duration = Duration::from_secs(1);

/// In-process wakeable scheduler; persisted state remains authoritative in SQLite.
#[derive(Clone)]
pub struct SchedulerService {
    wake: Arc<Notify>,
}

impl SchedulerService {
    /// Creates a scheduler handle before its background task is attached to Tauri.
    pub fn new() -> Self {
        Self {
            wake: Arc::new(Notify::new()),
        }
    }

    /// Starts one sequential worker, preventing overlapping changes on the same application.
    pub fn start(&self, app: AppHandle) {
        let wake = Arc::clone(&self.wake);
        let scheduler_app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(SCHEDULER_POLL_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = interval.tick() => {},
                    _ = wake.notified() => {},
                }
                if let Err(error) = run_due_schedules(&scheduler_app).await {
                    tracing::error!(%error, "scheduler iteration failed");
                }
            }
        });

        // A separate lightweight guard keeps Windows virtual desktops from restoring stale images.
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(WALLPAPER_RECONCILE_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let state = app.state::<AppState>();
                match state.platform.wallpaper.reconcile_wallpapers() {
                    Ok(restored) if restored > 0 => {
                        tracing::info!(restored, "platform wallpaper assignments reconciled");
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::debug!(%error, "platform wallpaper reconciliation skipped");
                    }
                }
            }
        });
    }

    /// Wakes the worker after configuration, resume, or an explicit Next request.
    pub fn wake(&self) {
        self.wake.notify_one();
    }
}

/// Executes each due monitor once; completion always recalculates from current time.
async fn run_due_schedules(app: &AppHandle) -> AppResult<()> {
    let state = app.state::<AppState>();
    let schedules = state.database.due_schedules()?;
    for schedule in schedules {
        execute_schedule(&state, &schedule).await;
    }
    Ok(())
}

/// Runs one complete selection/download/process/set transaction and persists its outcome.
async fn execute_schedule(state: &AppState, schedule: &ScheduleRecord) {
    let rules = match state.database.rotation_rules(&schedule.system_monitor_id) {
        Ok(rules) => rules,
        Err(error) => {
            record_failure(state, schedule, &error.to_string());
            return;
        }
    };
    let environment = if rules.pause_on_battery
        || rules.pause_on_fullscreen
        || rules.start_time.is_some()
        || rules.end_time.is_some()
        || rules.day_group != "all"
    {
        match state.platform.environment.runtime_environment() {
            Ok(environment) => environment,
            Err(error) => {
                record_failure(state, schedule, &error.to_string());
                return;
            }
        }
    } else {
        crate::platform::RuntimeEnvironment::default()
    };
    if let Some(reason) = rules.pause_reason(&environment) {
        if let Err(error) = state
            .database
            .defer_schedule_for_rule(&schedule.system_monitor_id, &reason)
        {
            tracing::error!(%error, "rotation rule deferral could not be persisted");
        }
        return;
    }
    let fit_mode = match FitMode::try_from(schedule.fit_mode.as_str()) {
        Ok(mode) => mode,
        Err(error) => {
            record_failure(state, schedule, &error.to_string());
            return;
        }
    };
    let service = WallpaperService::new(
        &state.database,
        &state.providers,
        &state.images,
        &state.platform,
        &state.paths.wallpapers_original_dir,
    );
    let mut failures = Vec::new();
    for attempt in 1..=3 {
        let wallpaper = match state
            .database
            .next_rotation_wallpaper(&schedule.system_monitor_id)
        {
            Ok(wallpaper) => wallpaper,
            Err(error) => {
                failures.push(error.to_string());
                break;
            }
        };
        match service
            .apply_to_monitor(wallpaper.id, &schedule.system_monitor_id, fit_mode, false)
            .await
        {
            Ok(processed) => {
                let retained = match state.database.wallpaper_by_hash(&processed.source_sha256) {
                    Ok(retained) => retained,
                    Err(error) => {
                        failures.push(error.to_string());
                        continue;
                    }
                };
                if let Err(error) = state.database.complete_schedule_run(
                    &schedule.system_monitor_id,
                    Some(retained.id),
                    None,
                ) {
                    tracing::error!(%error, monitor = %schedule.system_monitor_id, "scheduler history update failed");
                }
                let limit = state
                    .settings
                    .lock()
                    .map(|settings| settings.cache_limit_bytes);
                if let Ok(limit) = limit
                    && let Err(error) =
                        CacheService::new(state.database.clone(), state.paths.clone())
                            .enforce_limit(limit)
                {
                    tracing::warn!(%error, "scheduler cache enforcement failed");
                }
                return;
            }
            Err(error) => {
                failures.push(format!("attempt {attempt}: {error}"));
            }
        }
    }
    record_failure(state, schedule, &failures.join(" | "));
}

/// Stores a bounded error string and advances once, avoiding rapid failure loops after wake.
fn record_failure(state: &AppState, schedule: &ScheduleRecord, error: &str) {
    let bounded: String = error.chars().take(500).collect();
    if let Err(database_error) =
        state
            .database
            .complete_schedule_run(&schedule.system_monitor_id, None, Some(&bounded))
    {
        tracing::error!(%database_error, monitor = %schedule.system_monitor_id, "scheduler failure state could not be saved");
    }
    tracing::warn!(monitor = %schedule.system_monitor_id, error = %bounded, "scheduled wallpaper change failed");
}
