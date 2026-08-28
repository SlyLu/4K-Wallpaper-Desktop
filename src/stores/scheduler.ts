import { defineStore } from "pinia";
import { ref } from "vue";

import {
  configureWallpaperRotation,
  configureRotationPolicy,
  getRotationRules,
  getRotationExplanation,
  getSchedulerStatus,
  pauseScheduler,
  resumeScheduler,
  previousWallpaper,
  skipWallpaper,
  triggerNextWallpaper,
} from "../api/scheduler";
import type { FitMode } from "../models/image";
import type { RotationExplanation, RotationRules, RotationSelectionMode, RotationStrategy, ScheduleRecord } from "../models/scheduler";

const DEFAULT_RULES: RotationRules = { version: 1, dayGroup: "all", pauseOnBattery: false, pauseOnFullscreen: false };

export const useSchedulerStore = defineStore("scheduler", () => {
  const schedules = ref<ScheduleRecord[]>([]);
  const pending = ref(false);
  const error = ref("");
  const explanations = ref<Record<string, RotationExplanation>>({});
  const rules = ref<Record<string, RotationRules>>({});

  /** Refreshes persisted scheduler state after startup or a control action. */
  async function refresh(): Promise<void> {
    schedules.value = await getSchedulerStatus();
    const entries = await Promise.all(schedules.value.map(async (schedule) => {
      try {
        return [schedule.systemMonitorId, await getRotationExplanation(schedule.systemMonitorId)] as const;
      } catch {
        return undefined;
      }
    }));
    explanations.value = Object.fromEntries(entries.filter((entry): entry is readonly [string, RotationExplanation] => Boolean(entry)));
    const ruleEntries = await Promise.all(schedules.value.map(async (schedule) => [schedule.systemMonitorId, await getRotationRules(schedule.systemMonitorId)] as const));
    rules.value = Object.fromEntries(ruleEntries);
  }

  /** Configures one or more collection sources with a V2 selection strategy. */
  async function configurePolicy(
    monitorId: string,
    collectionIds: number[],
    intervalSeconds: number,
    fitMode: FitMode,
    strategy: RotationStrategy,
    rotationRules: RotationRules = DEFAULT_RULES,
  ): Promise<void> {
    await run(async () => {
      await configureRotationPolicy(monitorId, collectionIds, intervalSeconds, fitMode, strategy, rotationRules);
      await refresh();
    });
  }

  /** Configures a monitor-specific selected pool and requests its first change. */
  async function configure(
    monitorId: string,
    wallpaperIds: number[],
    intervalSeconds: number,
    fitMode: FitMode,
    selectionMode: RotationSelectionMode,
    rotationRules: RotationRules = DEFAULT_RULES,
  ): Promise<void> {
    await run(async () => {
      await configureWallpaperRotation(
        monitorId,
        wallpaperIds,
        intervalSeconds,
        fitMode,
        selectionMode,
        rotationRules,
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

  /** Applies the previous history item and refreshes the visible explanation. */
  async function previous(monitorId: string): Promise<void> {
    await run(async () => {
      await previousWallpaper(monitorId);
      await refresh();
    });
  }

  /** Skips the current candidate through the same scheduler wake-up path as Next. */
  async function skip(monitorId: string): Promise<void> {
    await run(async () => {
      await skipWallpaper(monitorId);
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

  return { schedules, explanations, rules, pending, error, refresh, configure, configurePolicy, setPaused, next, previous, skip };
});
