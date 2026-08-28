import type { ThemeManifest } from "../models/theme";

export const BUILTIN_THEMES: readonly ThemeManifest[] = [
  { schemaVersion: 1, id: "classic", name: "经典侧栏", navigation: "sidebar", appearance: "grid", density: "comfortable", radius: 16, shadow: "soft", motion: "standard", glass: false },
  { schemaVersion: 1, id: "gallery", name: "沉浸画廊", navigation: "dock", appearance: "immersive", density: "comfortable", radius: 22, shadow: "deep", motion: "standard", glass: true },
  { schemaVersion: 1, id: "compact", name: "紧凑管理", navigation: "top", appearance: "compact", density: "compact", radius: 10, shadow: "none", motion: "reduced", glass: false },
  { schemaVersion: 1, id: "glass", name: "玻璃工作台", navigation: "compact-sidebar", appearance: "glass", density: "comfortable", radius: 18, shadow: "deep", motion: "standard", glass: true },
] as const;

/** Runtime validation keeps future manifest loading inside the no-script schema boundary. */
export function isThemeManifest(value: unknown): value is ThemeManifest {
  if (!value || typeof value !== "object") return false;
  const theme = value as Partial<ThemeManifest>;
  return theme.schemaVersion === 1
    && ["classic", "gallery", "compact", "glass"].includes(theme.id ?? "")
    && ["sidebar", "compact-sidebar", "top", "dock"].includes(theme.navigation ?? "")
    && ["grid", "immersive", "compact", "glass"].includes(theme.appearance ?? "")
    && ["comfortable", "compact"].includes(theme.density ?? "")
    && typeof theme.radius === "number" && theme.radius >= 0 && theme.radius <= 32
    && ["none", "soft", "deep"].includes(theme.shadow ?? "")
    && ["reduced", "standard"].includes(theme.motion ?? "")
    && typeof theme.glass === "boolean";
}

/** Invalid or unknown manifests always fall back to the stable V1-compatible appearance. */
export function resolveTheme(id: string): ThemeManifest {
  const selected = BUILTIN_THEMES.find((theme) => theme.id === id);
  return selected && isThemeManifest(selected) ? selected : BUILTIN_THEMES[0];
}
