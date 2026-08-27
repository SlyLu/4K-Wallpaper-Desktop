import type { FitMode } from "./image";

export type RotationSelectionMode = "round_robin" | "random";

export interface ScheduleRecord {
  systemMonitorId: string;
  enabled: boolean;
  paused: boolean;
  intervalSeconds: number;
  fitMode: FitMode;
  lastChangeAt?: string;
  nextChangeAt: string;
  lastError?: string;
  wallpaperCount: number;
  selectionMode: RotationSelectionMode;
}
