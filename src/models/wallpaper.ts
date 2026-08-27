export interface WallpaperRecord {
  id: number;
  provider: string;
  remoteId: string;
  name: string;
  sourcePageUrl?: string;
  originalUrl?: string;
  thumbnailUrl?: string;
  thumbnailLocalPath?: string;
  localPath?: string;
  width: number;
  height: number;
  category: string;
  purity: string;
  mimeType?: string;
  fileSize?: number;
  aspectRatio?: string;
  hash?: string;
  downloadStatus: string;
  favorite: boolean;
  blacklisted: boolean;
  preset: boolean;
  tags: string[];
}

export interface WallpaperPage {
  items: WallpaperRecord[];
  page: number;
  pageSize: number;
  total: number;
}

export interface ThumbnailData {
  mimeType: string;
  bytes: number[];
}

export type CatalogSort = "latest" | "random" | "name";

export interface CatalogQuery {
  keyword?: string;
  name?: string;
  category?: "all" | "nature" | "anime" | "people" | "local";
  provider?: "all" | "wallhaven" | "local";
  /** Includes every original stored on this device without changing its provider identity. */
  locallyAvailable?: boolean;
  favorite?: boolean;
  minWidth?: number;
  minHeight?: number;
  includeBlacklisted?: boolean;
  sort?: CatalogSort;
  page?: number;
  pageSize?: number;
}

export type ProviderSort = "latest" | "popular" | "random";

export interface ProviderQuery {
  keyword?: string;
  category: "all" | "nature" | "anime" | "people" | "local";
  minWidth: number;
  minHeight: number;
  aspectRatio?: string;
  page: number;
  pageSize: number;
  sort: ProviderSort;
  safety: string;
}
