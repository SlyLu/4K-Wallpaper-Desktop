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
  fileAvailability: "remote" | "available" | "temporarily_unavailable" | "missing";
  storageKind: "remote_metadata" | "user_source" | "managed_download" | "processed" | "thumbnail";
  fileCopyCount: number;
  tags: string[];
}

export interface WallpaperPage {
  items: WallpaperRecord[];
  page: number;
  pageSize: number;
  total: number;
}

export interface DuplicateFileCopy {
  wallpaperId: number;
  path: string;
  storageKind: WallpaperRecord["storageKind"];
  availability: Exclude<WallpaperRecord["fileAvailability"], "remote">;
  fileSize?: number;
}

export interface DuplicateFileGroup {
  contentHash: string;
  copies: DuplicateFileCopy[];
}

export interface ThumbnailData {
  mimeType: string;
  bytes: number[];
}

export type CatalogSort = "latest" | "random" | "name";

export interface CatalogQuery {
  keyword?: string;
  name?: string;
  category?: "all" | "nature" | "anime" | "games" | "people" | "local";
  provider?: "all" | "wallhaven" | "wikimedia_commons" | "openverse" | "art_institute_chicago" | "thegamesdb" | "local";
  /** Includes every original stored on this device without changing its provider identity. */
  locallyAvailable?: boolean;
  fileBacked?: boolean;
  downloadStatus?: string;
  fileAvailability?: WallpaperRecord["fileAvailability"];
  storageKind?: WallpaperRecord["storageKind"];
  collectionId?: number;
  favorite?: boolean;
  minWidth?: number;
  minHeight?: number;
  maxWidth?: number;
  maxHeight?: number;
  aspectRatio?: string;
  mimeType?: string;
  tags?: string[];
  includeBlacklisted?: boolean;
  sort?: CatalogSort;
  page?: number;
  pageSize?: number;
}

export type ProviderSort = "latest" | "popular" | "random";

export interface ProviderQuery {
  keyword?: string;
  category: "all" | "nature" | "anime" | "games" | "people" | "local";
  minWidth: number;
  minHeight: number;
  aspectRatio?: string;
  page: number;
  pageSize: number;
  sort: ProviderSort;
  safety: string;
  /** Restricts online aggregation to the explicit source filter selected by the user. */
  providers?: string[];
}
