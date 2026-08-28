import { invoke } from "@tauri-apps/api/core";

import type { CollectionRecord, SmartCollectionRule } from "../models/collection";
import type { WallpaperPage } from "../models/wallpaper";

/** Lists collection summaries in persisted display order. */
export function listCollections(): Promise<CollectionRecord[]> {
  return invoke<CollectionRecord[]>("list_collections");
}
/** Creates one collection container; smart rules are attached separately. */
export function createCollection(name: string, description: string): Promise<CollectionRecord> {
  return invoke<CollectionRecord>("create_collection", { name, description });
}

/** Updates presentation fields without altering membership or wallpaper files. */
export function updateCollection(collection: CollectionRecord): Promise<CollectionRecord> {
  return invoke<CollectionRecord>("update_collection", {
    collectionId: collection.id,
    name: collection.name,
    description: collection.description,
    coverWallpaperId: collection.coverWallpaperId,
    position: collection.position,
  });
}

/** Deletes only one collection and its membership links. */
export function deleteCollection(collectionId: number): Promise<void> {
  return invoke("delete_collection", { collectionId });
}

/** Adds a cross-page batch to one manual collection. */
export function addCollectionWallpapers(collectionId: number, wallpaperIds: number[]): Promise<number> {
  return invoke<number>("add_collection_wallpapers", { collectionId, wallpaperIds });
}

/** Removes a cross-page batch from one manual collection. */
export function removeCollectionWallpapers(collectionId: number, wallpaperIds: number[]): Promise<number> {
  return invoke<number>("remove_collection_wallpapers", { collectionId, wallpaperIds });
}

/** Persists an allow-listed smart rule and returns its first preview. */
export function setSmartCollectionRule(collectionId: number, rule: SmartCollectionRule): Promise<WallpaperPage> {
  return invoke<WallpaperPage>("set_smart_collection_rule", { collectionId, rule });
}

/** Queries either manual membership or the saved smart rule. */
export function queryCollectionWallpapers(collectionId: number, page = 1, pageSize = 60): Promise<WallpaperPage> {
  return invoke<WallpaperPage>("query_collection_wallpapers", { collectionId, page, pageSize });
}
