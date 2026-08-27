import { invoke } from "@tauri-apps/api/core";

import type { FitMode, ImageMetadata, ProcessedImage } from "../models/image";

/** Reads trusted metadata after the Rust Core fully decodes the selected file. */
export function inspectImageFile(path: string): Promise<ImageMetadata> {
  return invoke<ImageMetadata>("inspect_image_file", { path });
}

/** Generates a proportional JPEG thumbnail inside the application cache. */
export function createThumbnail(
  path: string,
  maxWidth?: number,
  maxHeight?: number,
): Promise<ProcessedImage> {
  return invoke<ProcessedImage>("create_thumbnail", { path, maxWidth, maxHeight });
}

/** Renders a monitor-sized cache image using one of the four V1 adaptation modes. */
export function prepareWallpaperForMonitor(
  path: string,
  monitorId: string,
  fitMode: FitMode,
): Promise<ProcessedImage> {
  return invoke<ProcessedImage>("prepare_wallpaper_for_monitor", {
    path,
    monitorId,
    fitMode,
  });
}
