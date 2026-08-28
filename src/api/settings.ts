import { invoke } from "@tauri-apps/api/core";

import type { AppConfig } from "../models/settings";

export interface ThemeBackgroundData {
  path: string;
  mimeType: string;
  luminance: number;
  bytes: number[];
}

/** Reads the atomically persisted application configuration. */
export function getSettings(): Promise<AppConfig> {
  return invoke<AppConfig>("get_settings");
}

/** Validates and persists the complete V1 configuration. */
export function updateSettings(settings: AppConfig): Promise<AppConfig> {
  return invoke<AppConfig>("update_settings", { settings });
}

/** Imports a selected image into bounded AppData cache for safe WebView display. */
export function importThemeBackground(path: string): Promise<ThemeBackgroundData> {
  return invoke<ThemeBackgroundData>("import_theme_background", { path });
}

/** Loads an already imported background without granting arbitrary filesystem access. */
export function loadThemeBackground(path: string): Promise<ThemeBackgroundData> {
  return invoke<ThemeBackgroundData>("load_theme_background", { path });
}
