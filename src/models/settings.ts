export interface AppConfig {
  onlineProvider: string;
  minimumResolution: string;
  safety: string;
  resourceSyncEnabled: boolean;
  resourceSyncIntervalSeconds: number;
  wallpaperAutoChange: boolean;
  wallpaperChangeIntervalSeconds: number;
  wallpaperFitMode: string;
  cacheLimitBytes: number;
  closeToTray: boolean;
  autoStart: boolean;
  localDirectories: string[];
  themeMode: "dark" | "light" | "system" | "custom";
  themeEffect: "solid" | "gradient" | "rainbow";
  themeAccent: string;
  themeSecondary: string;
  themeBackground: string;
  themeSurface: string;
}
