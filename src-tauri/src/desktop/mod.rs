use tauri::{
    App, AppHandle, Manager,
    menu::MenuBuilder,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use crate::{
    AppState,
    error::{AppError, AppResult},
    image_processing::FitMode,
    wallpaper::WallpaperService,
};

/// Builds the required cross-platform tray menu with all actions routed through Core services.
pub fn setup_tray(app: &App) -> AppResult<()> {
    let menu = MenuBuilder::new(app)
        .text("open", "打开主窗口")
        .separator()
        .text("next", "下一张壁纸")
        .text("pause", "暂停自动切换")
        .text("resume", "恢复自动切换")
        .separator()
        .text("quit", "退出")
        .build()
        .map_err(|error| AppError::configuration(format!("tray menu failed: {error}")))?;
    let mut builder = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("4K Wallpaper Desktop")
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .build(app)
        .map_err(|error| AppError::configuration(format!("tray creation failed: {error}")))?;
    Ok(())
}

fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id.as_ref() {
        "open" => show_main_window(app),
        "next" => spawn_next_wallpapers(app.clone()),
        "pause" => set_all_paused(app, true),
        "resume" => set_all_paused(app, false),
        "quit" => app.exit(0),
        _ => tracing::debug!(id = %event.id.0, "unknown tray menu event ignored"),
    }
}

/// Restores and focuses the main window from the tray icon or menu.
pub(crate) fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(error) = window.show().and_then(|_| window.set_focus()) {
            tracing::warn!(%error, "main window could not be shown from tray");
        }
    }
}

/// Applies the next selected wallpaper to every configured display without changing pause state.
fn spawn_next_wallpapers(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let schedules = match state.database.list_schedules() {
            Ok(schedules) => schedules,
            Err(error) => {
                tracing::warn!(%error, "tray next could not load schedules");
                return;
            }
        };
        for schedule in schedules.into_iter().filter(|item| item.enabled) {
            let result = async {
                let wallpaper = state
                    .database
                    .next_rotation_wallpaper(&schedule.system_monitor_id)?;
                let fit_mode = FitMode::try_from(schedule.fit_mode.as_str())?;
                WallpaperService::new(
                    &state.database,
                    &state.providers,
                    &state.images,
                    &state.platform,
                    &state.paths.wallpapers_original_dir,
                )
                .apply_to_monitor(wallpaper.id, &schedule.system_monitor_id, fit_mode, false)
                .await?;
                state.database.complete_schedule_run(
                    &schedule.system_monitor_id,
                    Some(wallpaper.id),
                    None,
                )?;
                AppResult::Ok(())
            }
            .await;
            if let Err(error) = result {
                tracing::warn!(%error, monitor = %schedule.system_monitor_id, "tray next failed");
            }
        }
    });
}

/// Pauses or resumes every configured display while preserving its selected wallpaper pool.
fn set_all_paused(app: &AppHandle, paused: bool) {
    let state = app.state::<AppState>();
    match state.database.list_schedules() {
        Ok(schedules) => {
            for schedule in schedules {
                if let Err(error) = state
                    .database
                    .set_schedule_paused(&schedule.system_monitor_id, paused)
                {
                    tracing::warn!(%error, monitor = %schedule.system_monitor_id, "tray scheduler control failed");
                }
            }
            if !paused {
                state.scheduler.wake();
            }
        }
        Err(error) => tracing::warn!(%error, "tray scheduler control could not load schedules"),
    }
}
