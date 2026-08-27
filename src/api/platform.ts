import { invoke } from "@tauri-apps/api/core";

import type { AppStatus, MonitorInfo } from "../models/monitor";

/** Returns the initialized local paths and active platform adapter. */
export function getAppStatus(): Promise<AppStatus> {
  return invoke<AppStatus>("get_app_status");
}

/** Requests a fresh native monitor enumeration. */
export function getMonitors(): Promise<MonitorInfo[]> {
  return invoke<MonitorInfo[]>("get_monitors");
}

/** Applies one local image to every active monitor. */
export function setWallpaper(path: string): Promise<void> {
  return invoke("set_wallpaper", { path });
}

/** Applies one local image only to the selected native monitor identifier. */
export function setWallpaperForMonitor(path: string, monitorId: string): Promise<void> {
  return invoke("set_wallpaper_for_monitor", { path, monitorId });
}
