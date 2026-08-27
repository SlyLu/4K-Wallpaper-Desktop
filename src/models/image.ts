export type FitMode = "fill" | "fit" | "center" | "stretch";

export interface ImageMetadata {
  width: number;
  height: number;
  aspectRatio: string;
  fileSize: number;
  mimeType: string;
  format: string;
  sha256: string;
}

export interface ProcessedImage {
  path: string;
  width: number;
  height: number;
  sourceSha256: string;
  cacheHit: boolean;
}
