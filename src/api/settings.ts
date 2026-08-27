import { invoke } from "@tauri-apps/api/core";

import type { AppConfig } from "../models/settings";

/** Reads the atomically persisted application configuration. */
export function getSettings(): Promise<AppConfig> {
  return invoke<AppConfig>("get_settings");
}

/** Validates and persists the complete V1 configuration. */
export function updateSettings(settings: AppConfig): Promise<AppConfig> {
  return invoke<AppConfig>("update_settings", { settings });
}
