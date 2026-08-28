import type { AppConfig } from "../models/settings";
import { resolveTheme } from "../themes/builtin";

const COLOR_FIELDS = [
  "--color-accent",
  "--color-accent-2",
  "--color-bg",
  "--color-surface",
  "--color-text",
  "--color-on-accent",
] as const;

/** Applies persisted theme tokens without coupling visual preferences to Rust business logic. */
export function applyTheme(settings: AppConfig, backgroundUrl?: string, luminance?: number): void {
  const root = document.documentElement;
  const manifest = resolveTheme(settings.themePack);
  const systemMode = window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  const resolvedMode = settings.themeMode === "system" ? systemMode : settings.themeMode;
  root.dataset.theme = resolvedMode;
  // System mode keeps the built-in palette/effect while V2 may safely overlay a local image.
  root.dataset.themeEffect = settings.themeMode === "system" ? "solid" : settings.themeEffect;
  root.style.colorScheme = resolvedMode === "light" ? "light" : "dark";
  root.dataset.navigation = manifest.navigation;
  root.dataset.appearance = manifest.appearance;
  root.dataset.density = manifest.density;
  root.dataset.shadow = manifest.shadow;
  root.dataset.motion = manifest.motion;
  root.dataset.glass = String(manifest.glass);
  const overlayBackground = resolvedMode === "custom"
    ? settings.themeBackground
    : resolvedMode === "light" ? "#eef6fb" : "#07111d";
  const effectiveLuminance = luminance === undefined
    ? colorLuminance(overlayBackground)
    : luminance * (1 - settings.themeBackgroundOverlay)
      + colorLuminance(overlayBackground) * settings.themeBackgroundOverlay;
  root.dataset.backgroundTone = effectiveLuminance > 0.58 ? "light" : "dark";
  root.style.setProperty("--theme-radius", `${manifest.radius}px`);
  root.style.setProperty("--theme-background-overlay", String(settings.themeBackgroundOverlay));
  applyBackground(root, settings, backgroundUrl);

  if (resolvedMode !== "custom") {
    COLOR_FIELDS.forEach((name) => root.style.removeProperty(name));
    return;
  }
  root.style.setProperty("--color-accent", settings.themeAccent);
  root.style.setProperty("--color-accent-2", settings.themeSecondary);
  root.style.setProperty("--color-bg", settings.themeBackground);
  root.style.setProperty("--color-surface", settings.themeSurface);
  root.style.setProperty("--color-text", readableText(settings.themeBackground));
  root.style.setProperty("--color-on-accent", readableText(settings.themeAccent));
  if (backgroundUrl && luminance !== undefined) {
    root.style.setProperty("--color-text", effectiveLuminance > 0.58 ? "#102231" : "#f4f9fd");
  }
}

/** Maps validated local background settings to CSS tokens without exposing a filesystem URL. */
function applyBackground(root: HTMLElement, settings: AppConfig, backgroundUrl?: string): void {
  if (!backgroundUrl) {
    root.dataset.hasBackground = "false";
    root.style.removeProperty("--theme-background-image");
    return;
  }
  const sizing = {
    fill: "cover",
    fit: "contain",
    center: "auto",
    stretch: "100% 100%",
  }[settings.themeBackgroundFit];
  root.dataset.hasBackground = "true";
  root.style.setProperty("--theme-background-image", `url("${backgroundUrl}")`);
  root.style.setProperty("--theme-background-size", sizing);
}

/** Chooses black or white foreground from relative luminance for custom color readability. */
export function readableText(hex: string): string {
  return colorLuminance(hex) > 0.58 ? "#07111d" : "#f7fbff";
}

/** Returns a bounded luminance estimate shared by live and preview contrast decisions. */
export function colorLuminance(hex: string): number {
  const normalized = hex.replace("#", "");
  if (!/^[0-9a-f]{6}$/i.test(normalized)) return 0;
  const [red, green, blue] = [0, 2, 4].map((offset) => Number.parseInt(normalized.slice(offset, offset + 2), 16));
  return (red * 299 + green * 587 + blue * 114) / 255000;
}
