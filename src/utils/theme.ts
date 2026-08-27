import type { AppConfig } from "../models/settings";

const COLOR_FIELDS = [
  "--color-accent",
  "--color-accent-2",
  "--color-bg",
  "--color-surface",
  "--color-text",
  "--color-on-accent",
] as const;

/** Applies persisted theme tokens without coupling visual preferences to Rust business logic. */
export function applyTheme(settings: AppConfig): void {
  const root = document.documentElement;
  const systemMode = window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  const resolvedMode = settings.themeMode === "system" ? systemMode : settings.themeMode;
  root.dataset.theme = resolvedMode;
  // System mode deliberately uses the built-in solid palette; custom backgrounds belong to V2.
  root.dataset.themeEffect = settings.themeMode === "system" ? "solid" : settings.themeEffect;
  root.style.colorScheme = resolvedMode === "light" ? "light" : "dark";

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
}

/** Chooses black or white foreground from relative luminance for custom color readability. */
export function readableText(hex: string): string {
  const normalized = hex.replace("#", "");
  if (!/^[0-9a-f]{6}$/i.test(normalized)) return "#edf6ff";
  const [red, green, blue] = [0, 2, 4].map((offset) => Number.parseInt(normalized.slice(offset, offset + 2), 16));
  const luminance = (red * 299 + green * 587 + blue * 114) / 255000;
  return luminance > 0.58 ? "#07111d" : "#f7fbff";
}
