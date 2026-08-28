export interface ProviderStatus {
  provider: string;
  enabled: boolean;
  status: "unknown" | "healthy" | "degraded" | "unavailable";
  lastSuccessAt?: string;
  lastErrorAt?: string;
  lastError?: string;
  responseTimeMs?: number;
}

/** Attribution for every remote provider represented by one deduplicated card. */
export interface WallpaperProviderSource {
  provider: string;
  remoteId: string;
  sourcePageUrl?: string;
  originalUrl?: string;
  author?: string;
  licenseName?: string;
  licenseUrl?: string;
  width?: number;
  height?: number;
}
