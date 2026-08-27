import { defineStore } from "pinia";
import { ref } from "vue";

import {
  configureWallpaperRotation,
  getSchedulerStatus,
  pauseScheduler,
  resumeScheduler,
  triggerNextWallpaper,
} from "../api/scheduler";
import type { FitMode } from "../models/image";
import type { RotationSelectionMode, ScheduleRecord } from "../models/scheduler";

export const useSchedulerStore = defineStore("scheduler", () => {
  const schedules = ref<ScheduleRecord[]>([]);
  const pending = ref(false);
  const error = ref("");

  /** Refreshes persisted scheduler state after startup or a control action. */
  async function refresh(): Promise<void> {
    schedules.value = await getSchedulerStatus();
  }

  /** Configures a monitor-specific selected pool and requests its first change. */
  async function configure(
    monitorId: string,
    wallpaperIds: number[],
    intervalSeconds: number,
    fitMode: FitMode,
    selectionMode: RotationSelectionMode,
  ): Promise<void> {
    await run(async () => {
      await configureWallpaperRotation(
        monitorId,
        wallpaperIds,
        intervalSeconds,
        fitMode,
        selectionMode,
      );
      await refresh();
    });
  }

  /** Pauses or resumes one monitor while retaining its pool and interval. */
  async function setPaused(monitorId: string, paused: boolean): Promise<void> {
    await run(async () => {
      await (paused ? pauseScheduler(monitorId) : resumeScheduler(monitorId));
      await refresh();
    });
  }

  /** Requests an immediate next wallpaper and then refreshes visible state. */
  async function next(monitorId: string): Promise<void> {
    await run(async () => {
      await triggerNextWallpaper(monitorId);
      await refresh();
    });
  }

  /** Serializes UI mutations and exposes one concise error channel. */
  async function run(operation: () => Promise<void>): Promise<void> {
    pending.value = true;
    error.value = "";
    try {
      await operation();
    } catch (cause) {
      error.value = String(cause);
      throw cause;
    } finally {
      pending.value = false;
    }
  }

  return { schedules, pending, error, refresh, configure, setPaused, next };
});
