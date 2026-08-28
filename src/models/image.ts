export type FitMode = "fill" | "fit" | "center" | "stretch";

export interface ImageMetadata {
  width: number;
  height: number;
  aspectRatio: string;
  fileSize: number;
  mimeType: string;
  format: string;
  sha256: string;
  perceptualHash: string;
}

export interface ProcessedImage {
  path: string;
  width: number;
  height: number;
  sourceSha256: string;
  cacheHit: boolean;
}

export interface SpanningSliceImage {
  systemMonitorId: string;
  path: string;
  width: number;
  height: number;
  cacheHit: boolean;
}
