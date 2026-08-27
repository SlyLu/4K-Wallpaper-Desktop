import { invoke } from "@tauri-apps/api/core";

import type { CacheCleanupResult, CacheInfo } from "../models/cache";

/** Reads application-owned cache usage grouped by file purpose. */
export function getCacheInfo(): Promise<CacheInfo> {
  return invoke<CacheInfo>("get_cache_info");
}

/** Clears removable cache entries while Rust protects favorites and LocalProvider files. */
export function clearCache(): Promise<CacheCleanupResult> {
  return invoke<CacheCleanupResult>("clear_cache");
}
