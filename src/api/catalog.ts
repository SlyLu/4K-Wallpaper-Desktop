import { invoke } from "@tauri-apps/api/core";

import type { FitMode, ProcessedImage } from "../models/image";
import type {
  CatalogQuery,
  ProviderQuery,
  ThumbnailData,
  WallpaperPage,
  WallpaperRecord,
} from "../models/wallpaper";

/** Loads one bounded page from the local SQLite catalog. */
export function listPresetWallpapers(page = 1, pageSize = 12): Promise<WallpaperPage> {
  return invoke<WallpaperPage>("list_wallpapers", {
    page,
    pageSize,
    presetOnly: true,
  });
}

/** Queries the provider-neutral SQLite metadata index used by every catalog page. */
export function queryCatalog(query: CatalogQuery): Promise<WallpaperPage> {
  return invoke<WallpaperPage>("query_catalog", { query });
}

/** Synchronizes one bounded Wallhaven metadata page without downloading originals. */
export function syncCatalog(query: ProviderQuery): Promise<number> {
  return invoke<number>("sync_catalog", { query });
}

/** Lets startup refresh metadata only when the persisted 24-hour interval is due. */
export function syncCatalogIfDue(): Promise<number> {
  return invoke<number>("sync_catalog_if_due");
}

/** Indexes a user-selected local directory while leaving originals untouched. */
export function scanLocalDirectory(path: string): Promise<number> {
  return invoke<number>("scan_local_directory", { path });
}

/** Imports explicitly dropped files or folders through LocalProvider validation. */
export function importLocalPaths(paths: string[]): Promise<number> {
  return invoke<number>("import_local_paths", { paths });
}

/** Removes a LocalProvider database index without deleting the source file. */
export function removeLocalWallpaper(wallpaperId: number): Promise<void> {
  return invoke("remove_local_wallpaper", { wallpaperId });
}

/** Removes LocalProvider indexes whose source files no longer exist. */
export function pruneMissingLocalWallpapers(): Promise<number> {
  return invoke<number>("prune_missing_local_wallpapers");
}

/** Removes a directory from settings without deleting user-owned files. */
export function removeLocalDirectory(path: string): Promise<unknown> {
  return invoke("remove_local_directory", { path });
}

/** Reads a trusted cached thumbnail through Rust instead of widening filesystem access. */
export function getWallpaperThumbnail(wallpaperId: number): Promise<ThumbnailData> {
  return invoke<ThumbnailData>("get_wallpaper_thumbnail", { wallpaperId });
}

/** Downloads, validates, and content-deduplicates one 4K original. */
export function downloadWallpaper(wallpaperId: number): Promise<WallpaperRecord> {
  return invoke<WallpaperRecord>("download_wallpaper", { wallpaperId });
}

/** Reads raw original bytes without JSON-expanding a multi-megabyte image. */
export async function getWallpaperOriginalBytes(wallpaperId: number): Promise<ArrayBuffer> {
  const response = await invoke<ArrayBuffer | Uint8Array | number[]>(
    "get_wallpaper_original_bytes",
    { wallpaperId },
  );
  if (response instanceof ArrayBuffer) return response;
  return Uint8Array.from(response).buffer;
}

/** Runs the complete manual download/process/set/history workflow. */
export function applyCatalogWallpaper(
  wallpaperId: number,
  monitorId: string,
  fitMode: FitMode,
): Promise<ProcessedImage> {
  return invoke<ProcessedImage>("apply_catalog_wallpaper", {
    wallpaperId,
    monitorId,
    fitMode,
  });
}

/** Persists favorite state and returns refreshed metadata. */
export function setWallpaperFavorite(
  wallpaperId: number,
  favorite: boolean,
): Promise<WallpaperRecord> {
  return invoke<WallpaperRecord>("set_wallpaper_favorite", { wallpaperId, favorite });
}

/** Persists blacklist state; blacklisted items are removed from rotation pools. */
export function setWallpaperBlacklisted(
  wallpaperId: number,
  blacklisted: boolean,
): Promise<WallpaperRecord> {
  return invoke<WallpaperRecord>("set_wallpaper_blacklisted", {
    wallpaperId,
    blacklisted,
  });
}

/** Deletes one application-owned, non-favorite remote original cache entry. */
export function deleteWallpaperCache(wallpaperId: number): Promise<WallpaperRecord> {
  return invoke<WallpaperRecord>("delete_wallpaper_cache", { wallpaperId });
}
