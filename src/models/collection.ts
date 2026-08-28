import type { WallpaperPage, WallpaperRecord } from "./wallpaper";

export interface CollectionRecord {
  id: number;
  name: string;
  description: string;
  coverWallpaperId?: number;
  position: number;
  wallpaperCount: number;
  smart: boolean;
}

export interface SmartCollectionRule {
  version: 1;
  provider?: string;
  category?: string;
  favorite?: boolean;
  fileAvailability?: WallpaperRecord["fileAvailability"];
  minWidth?: number;
  minHeight?: number;
  aspectRatio?: string;
  tags: string[];
}

export type CollectionWallpaperPage = WallpaperPage;
