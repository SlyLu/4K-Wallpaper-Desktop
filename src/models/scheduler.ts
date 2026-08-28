import type { FitMode } from "./image";

export type RotationSelectionMode = "round_robin" | "random";
export type RotationStrategy = "round_robin" | "shuffle" | "least_recent" | "weighted_random";

export interface RotationRules {
  version: 1;
  startTime?: string;
  endTime?: string;
  dayGroup: "all" | "weekdays" | "weekends";
  pauseOnBattery: boolean;
  pauseOnFullscreen: boolean;
}

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

export interface RotationExplanation {
  systemMonitorId: string;
  strategy: RotationStrategy;
  lastReason?: string;
  sourceCollectionCount: number;
  sourceCollectionIds: number[];
  candidateCount: number;
  queuedCount: number;
}
