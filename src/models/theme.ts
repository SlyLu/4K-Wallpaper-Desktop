export type ThemeNavigation = "sidebar" | "compact-sidebar" | "top" | "dock";
export type ThemeAppearance = "grid" | "immersive" | "compact" | "glass";

/** Declarative-only built-in manifest; no executable hooks or remote resources are allowed. */
export interface ThemeManifest {
  schemaVersion: 1;
  id: "classic" | "gallery" | "compact" | "glass";
  name: string;
  navigation: ThemeNavigation;
  appearance: ThemeAppearance;
  density: "comfortable" | "compact";
  radius: number;
  shadow: "none" | "soft" | "deep";
  motion: "reduced" | "standard";
  glass: boolean;
}
