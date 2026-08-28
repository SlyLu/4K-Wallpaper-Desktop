import { invoke } from "@tauri-apps/api/core";

import type { ProviderStatus, WallpaperProviderSource } from "../models/provider";

/** Lists independent built-in provider configuration and health state. */
export function listProviders(): Promise<ProviderStatus[]> {
  return invoke<ProviderStatus[]>("list_providers");
}

/** Toggles one provider without selecting a global default source. */
export function updateProviderConfig(provider: string, enabled: boolean): Promise<ProviderStatus[]> {
  return invoke<ProviderStatus[]>("update_provider_config", { provider, enabled });
}

/** Loads all provider links and licensing metadata retained after cross-source deduplication. */
export function listWallpaperSources(wallpaperId: number): Promise<WallpaperProviderSource[]> {
  return invoke<WallpaperProviderSource[]>("list_wallpaper_sources", { wallpaperId });
}
