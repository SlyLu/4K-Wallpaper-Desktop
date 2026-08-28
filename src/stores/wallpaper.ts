import { defineStore } from "pinia";
import { computed, ref } from "vue";

import {
  applyCatalogWallpaper,
  deleteWallpaperCache,
  downloadWallpaper,
  getWallpaperOriginalBytes,
  getWallpaperThumbnail,
  queryCatalog,
  setWallpaperBlacklisted,
  setWallpaperFavorite,
  syncCatalog,
} from "../api/catalog";
import type { FitMode } from "../models/image";
import type { CatalogQuery, ProviderQuery, WallpaperRecord } from "../models/wallpaper";
import { queryCollectionWallpapers } from "../api/collections";

export const useWallpaperStore = defineStore("wallpaper", () => {
  const wallpapers = ref<WallpaperRecord[]>([]);
  const thumbnailUrls = ref<Record<number, string>>({});
  const selectedIds = ref<number[]>([]);
  const bulkSelections = ref<Record<number, WallpaperRecord>>({});
  const activeWallpaper = ref<WallpaperRecord>();
  const total = ref(0);
  const page = ref(1);
  const pageSize = ref(30);
  const loading = ref(false);
  const syncing = ref(false);
  const error = ref("");
  const lastQuery = ref<CatalogQuery>({ page: 1, pageSize: 30 });
  const activeCollectionId = ref<number>();

  const selectedCount = computed(() => selectedIds.value.length);
  const bulkSelectedCount = computed(() => Object.keys(bulkSelections.value).length);

  /** Loads one bounded SQLite page and resolves only its visible cached thumbnails. */
  async function query(filters: CatalogQuery = {}): Promise<void> {
    loading.value = true;
    error.value = "";
    lastQuery.value = { page: 1, pageSize: 30, ...filters };
    activeCollectionId.value = undefined;
    try {
      releaseThumbnails();
      const result = await queryCatalog(lastQuery.value);
      wallpapers.value = result.items;
      total.value = result.total;
      page.value = result.page;
      pageSize.value = result.pageSize;
      await Promise.all(result.items.map(loadCachedThumbnail));
    } catch (cause) {
      error.value = String(cause);
    } finally {
      loading.value = false;
    }
  }

  /** Loads one manual or smart collection while retaining its paging context. */
  async function queryCollection(collectionId: number, targetPage = 1): Promise<void> {
    loading.value = true;
    error.value = "";
    activeCollectionId.value = collectionId;
    try {
      releaseThumbnails();
      const result = await queryCollectionWallpapers(collectionId, targetPage, pageSize.value || 60);
      wallpapers.value = result.items;
      total.value = result.total;
      page.value = result.page;
      pageSize.value = result.pageSize;
      await Promise.all(result.items.map(loadCachedThumbnail));
    } catch (cause) {
      error.value = String(cause);
    } finally {
      loading.value = false;
    }
  }

  /** Retains the validation-screen entry point as a general catalog query. */
  function load(): Promise<void> {
    return query({ page: 1, pageSize: 30 });
  }

  /** Loads another bounded SQLite page while preserving the active filters and sort order. */
  function goToPage(targetPage: number): Promise<void> {
    const totalPages = Math.max(1, Math.ceil(total.value / pageSize.value));
    const requestedPage = Number.isFinite(targetPage) ? Math.trunc(targetPage) : 1;
    const safePage = Math.min(Math.max(1, requestedPage), totalPages);
    return activeCollectionId.value
      ? queryCollection(activeCollectionId.value, safePage)
      : query({ ...lastQuery.value, page: safePage });
  }

  /** Fetches metadata only, then refreshes the local catalog using active filters. */
  async function syncOnline(providerQuery: ProviderQuery): Promise<number> {
    syncing.value = true;
    error.value = "";
    try {
      const imported = await syncCatalog(providerQuery);
      await query(lastQuery.value);
      return imported;
    } catch (cause) {
      error.value = String(cause);
      throw cause;
    } finally {
      syncing.value = false;
    }
  }

  /** Uses a local Blob URL when available and a remote thumbnail URL otherwise. */
  function thumbnailFor(wallpaper: WallpaperRecord): string {
    return thumbnailUrls.value[wallpaper.id] ?? wallpaper.thumbnailUrl ?? "";
  }

  /** Adds or removes one card from the explicit automatic-rotation selection. */
  function toggleSelected(wallpaperId: number): void {
    selectedIds.value = selectedIds.value.includes(wallpaperId)
      ? selectedIds.value.filter((id) => id !== wallpaperId)
      : [...selectedIds.value, wallpaperId];
  }

  /** Adds or removes one record from a cross-page batch without mixing it with rotation selection. */
  function toggleBulkSelected(wallpaper: WallpaperRecord): void {
    const next = { ...bulkSelections.value };
    if (next[wallpaper.id]) delete next[wallpaper.id];
    else next[wallpaper.id] = wallpaper;
    bulkSelections.value = next;
  }

  /** Selects or clears only the visible page while retaining selections made on other pages. */
  function setCurrentPageBulkSelected(selected: boolean): void {
    const next = { ...bulkSelections.value };
    for (const wallpaper of wallpapers.value) {
      if (selected) next[wallpaper.id] = wallpaper;
      else delete next[wallpaper.id];
    }
    bulkSelections.value = next;
  }

  /** Clears temporary batch state when leaving a catalog management page. */
  function clearBulkSelected(): void {
    bulkSelections.value = {};
  }

  /** Applies safe removal semantics and refreshes the current filtered page afterwards. */
  async function removeBulkSelected(mode: "dislike" | "index" | "gallery"): Promise<{ removed: number; failed: number }> {
    const records = Object.values(bulkSelections.value);
    let removed = 0;
    let failed = 0;
    for (const wallpaper of records) {
      try {
        if (mode === "gallery" && wallpaper.provider !== "local") {
          // Explicit gallery removal may unprotect a favorite before deleting app-owned cache.
          if (wallpaper.favorite) await setWallpaperFavorite(wallpaper.id, false);
          await deleteWallpaperCache(wallpaper.id);
        } else {
          // A retained blacklist tombstone prevents online refresh or local rescans from resurrecting it.
          if (wallpaper.provider !== "local" && wallpaper.localPath) {
            if (wallpaper.favorite) await setWallpaperFavorite(wallpaper.id, false);
            await deleteWallpaperCache(wallpaper.id);
          }
          await setWallpaperBlacklisted(wallpaper.id, true);
        }
        selectedIds.value = selectedIds.value.filter((id) => id !== wallpaper.id);
        removed += 1;
      } catch {
        failed += 1;
      }
    }
    clearBulkSelected();
    await query(lastQuery.value);
    if (!wallpapers.value.length && page.value > 1) await goToPage(page.value - 1);
    return { removed, failed };
  }

  /** Downloads one original and updates catalog state without reloading every thumbnail. */
  async function downloadOriginal(wallpaperId: number): Promise<{
    wallpaper: WallpaperRecord;
    bytes: ArrayBuffer;
  }> {
    const wallpaper = await downloadWallpaper(wallpaperId);
    replaceWallpaper(wallpaper, wallpaperId);
    const bytes = await getWallpaperOriginalBytes(wallpaper.id);
    return { wallpaper, bytes };
  }

  /** Applies one catalog item through the complete Rust Core workflow. */
  async function apply(wallpaperId: number, monitorId: string, fitMode: FitMode): Promise<void> {
    const result = await applyCatalogWallpaper(wallpaperId, monitorId, fitMode);
    replaceWallpaper(result.wallpaper, wallpaperId);
  }

  /** Toggles favorite state and keeps list/detail views consistent. */
  async function toggleFavorite(wallpaper: WallpaperRecord): Promise<WallpaperRecord> {
    const updated = await setWallpaperFavorite(wallpaper.id, !wallpaper.favorite);
    if (lastQuery.value.favorite === true && !updated.favorite) {
      // Favorites is a filtered view, so cancellation must remove the card immediately.
      wallpapers.value = wallpapers.value.filter((item) => item.id !== updated.id);
      total.value = Math.max(0, total.value - 1);
      if (activeWallpaper.value?.id === updated.id) activeWallpaper.value = updated;
      if (!wallpapers.value.length && page.value > 1) await goToPage(page.value - 1);
    } else {
      replaceWallpaper(updated);
    }
    return updated;
  }

  /** Blacklists one item and removes it from visible and selected collections. */
  async function blacklist(wallpaperId: number): Promise<void> {
    await setWallpaperBlacklisted(wallpaperId, true);
    wallpapers.value = wallpapers.value.filter((wallpaper) => wallpaper.id !== wallpaperId);
    selectedIds.value = selectedIds.value.filter((id) => id !== wallpaperId);
    total.value = Math.max(0, total.value - 1);
    if (activeWallpaper.value?.id === wallpaperId) activeWallpaper.value = undefined;
  }

  /** Removes only a safe application-owned remote original cache entry. */
  async function deleteCache(wallpaperId: number): Promise<void> {
    replaceWallpaper(await deleteWallpaperCache(wallpaperId));
  }

  /** Removes an item from the device gallery while preserving user-owned local files. */
  async function removeFromLibrary(wallpaper: WallpaperRecord): Promise<void> {
    if (wallpaper.provider === "local") {
      // Keep a hidden tombstone so a tracked-directory refresh does not re-add the item.
      await setWallpaperBlacklisted(wallpaper.id, true);
    } else {
      if (wallpaper.favorite) await setWallpaperFavorite(wallpaper.id, false);
      await deleteWallpaperCache(wallpaper.id);
    }
    wallpapers.value = wallpapers.value.filter((item) => item.id !== wallpaper.id);
    selectedIds.value = selectedIds.value.filter((id) => id !== wallpaper.id);
    if (activeWallpaper.value?.id === wallpaper.id) activeWallpaper.value = undefined;
    total.value = Math.max(0, total.value - 1);
  }

  /** Replaces one immutable catalog record returned by the backend. */
  function replaceWallpaper(updated: WallpaperRecord, replacedId = updated.id): void {
    wallpapers.value = wallpapers.value
      .filter((wallpaper) => wallpaper.id !== replacedId || replacedId === updated.id)
      .map((wallpaper) => wallpaper.id === updated.id ? updated : wallpaper);
    if (activeWallpaper.value?.id === replacedId || activeWallpaper.value?.id === updated.id) {
      activeWallpaper.value = updated;
    }
  }

  /** Resolves cache bytes only when Rust reports a trusted local thumbnail. */
  async function loadCachedThumbnail(wallpaper: WallpaperRecord): Promise<void> {
    if (!wallpaper.thumbnailLocalPath) return;
    try {
      const data = await getWallpaperThumbnail(wallpaper.id);
      const blob = new Blob([new Uint8Array(data.bytes)], { type: data.mimeType });
      thumbnailUrls.value = { ...thumbnailUrls.value, [wallpaper.id]: URL.createObjectURL(blob) };
    } catch {
      // Online items retain a remote thumbnail URL when the optional cache is unavailable.
    }
  }

  /** Revokes every Blob URL created by this store to avoid retaining image memory. */
  function releaseThumbnails(): void {
    Object.values(thumbnailUrls.value).forEach((url) => URL.revokeObjectURL(url));
    thumbnailUrls.value = {};
  }

  return {
    wallpapers,
    thumbnailUrls,
    selectedIds,
    bulkSelections,
    activeWallpaper,
    selectedCount,
    bulkSelectedCount,
    total,
    page,
    pageSize,
    loading,
    syncing,
    error,
    query,
    queryCollection,
    load,
    goToPage,
    syncOnline,
    thumbnailFor,
    toggleSelected,
    toggleBulkSelected,
    setCurrentPageBulkSelected,
    clearBulkSelected,
    removeBulkSelected,
    downloadOriginal,
    apply,
    toggleFavorite,
    blacklist,
    deleteCache,
    removeFromLibrary,
    releaseThumbnails,
  };
});
