export interface CacheInfo {
  totalBytes: number;
  limitBytes: number;
  originalBytes: number;
  thumbnailBytes: number;
  processedBytes: number;
  fileCount: number;
}

export interface CacheCleanupResult {
  beforeBytes: number;
  afterBytes: number;
  freedBytes: number;
  removedFiles: number;
  limitBytes: number;
}
