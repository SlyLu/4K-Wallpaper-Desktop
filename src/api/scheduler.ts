import { invoke } from "@tauri-apps/api/core";

import type { FitMode } from "../models/image";
import type { RotationExplanation, RotationRules, RotationSelectionMode, RotationStrategy, ScheduleRecord } from "../models/scheduler";

/** Replaces one monitor's selected pool and enables automatic rotation. */
export function configureWallpaperRotation(
  monitorId: string,
  wallpaperIds: number[],
  intervalSeconds: number,
  fitMode: FitMode,
  selectionMode: RotationSelectionMode,
  rules?: RotationRules,
): Promise<ScheduleRecord> {
  return invoke<ScheduleRecord>("configure_wallpaper_rotation", {
    monitorId,
    wallpaperIds,
    intervalSeconds,
    fitMode,
    selectionMode,
    rules,
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

/** Configures collection-backed V2 rotation and its persisted selection strategy. */
export function configureRotationPolicy(
  monitorId: string,
  collectionIds: number[],
  intervalSeconds: number,
  fitMode: FitMode,
  strategy: RotationStrategy,
  rules: RotationRules,
): Promise<ScheduleRecord> {
  return invoke<ScheduleRecord>("configure_rotation_policy", {
    monitorId,
    collectionIds,
    intervalSeconds,
    fitMode,
    strategy,
    rules,
  });
}

/** Restores one display's validated rule form after route navigation or restart. */
export function getRotationRules(monitorId: string): Promise<RotationRules> {
  return invoke<RotationRules>("get_rotation_rules", { monitorId });
}

/** Reads the persisted reason and queue state for one monitor. */
export function getRotationExplanation(monitorId: string): Promise<RotationExplanation> {
  return invoke<RotationExplanation>("get_rotation_explanation", { monitorId });
}

/** Applies the most recent different history item for one monitor. */
export function previousWallpaper(monitorId: string): Promise<unknown> {
  return invoke("previous_wallpaper", { monitorId });
}

/** Skips the current candidate and wakes the in-process scheduler. */
export function skipWallpaper(monitorId: string): Promise<void> {
  return invoke("skip_wallpaper", { monitorId });
}
