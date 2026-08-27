import { invoke } from "@tauri-apps/api/core";

import type { FitMode } from "../models/image";
import type { RotationSelectionMode, ScheduleRecord } from "../models/scheduler";

/** Replaces one monitor's selected pool and enables automatic rotation. */
export function configureWallpaperRotation(
  monitorId: string,
  wallpaperIds: number[],
  intervalSeconds: number,
  fitMode: FitMode,
  selectionMode: RotationSelectionMode,
): Promise<ScheduleRecord> {
  return invoke<ScheduleRecord>("configure_wallpaper_rotation", {
    monitorId,
    wallpaperIds,
    intervalSeconds,
    fitMode,
    selectionMode,
  });
}

/** Returns persisted state for every configured monitor. */
export function getSchedulerStatus(): Promise<ScheduleRecord[]> {
  return invoke<ScheduleRecord[]>("get_scheduler_status");
}

/** Pauses one monitor without discarding its selected pool. */
export function pauseScheduler(monitorId: string): Promise<ScheduleRecord> {
  return invoke<ScheduleRecord>("pause_scheduler", { monitorId });
}

/** Resumes one monitor and schedules one immediate catch-up run. */
export function resumeScheduler(monitorId: string): Promise<ScheduleRecord> {
  return invoke<ScheduleRecord>("resume_scheduler", { monitorId });
}

/** Requests one immediate change from the selected pool or recent-history fallback. */
export function triggerNextWallpaper(monitorId: string): Promise<void> {
  return invoke("trigger_next_wallpaper", { monitorId });
}
