import { defineStore } from "pinia";
import { ref } from "vue";

import { getSettings, importThemeBackground, loadThemeBackground, updateSettings } from "../api/settings";
import type { AppConfig } from "../models/settings";

export const useSettingsStore = defineStore("settings", () => {
  const settings = ref<AppConfig>();
  const loading = ref(false);
  const error = ref("");
  const backgroundUrl = ref("");
  const backgroundLuminance = ref<number>();

  /** Loads the single shared configuration instead of letting pages read files independently. */
  async function load(): Promise<void> {
    loading.value = true;
    error.value = "";
    try {
      settings.value = await getSettings();
      await reloadBackground();
    } catch (cause) {
      error.value = String(cause);
    } finally {
      loading.value = false;
    }
  }

  /** Imports and previews one local background while retaining only its AppData copy. */
  async function importBackground(path: string): Promise<string> {
    const data = await importThemeBackground(path);
    replaceBackgroundBlob(data.bytes, data.mimeType, data.luminance);
    return data.path;
  }

  /** Missing cached backgrounds safely clear visual state without blocking settings startup. */
  async function reloadBackground(): Promise<void> {
    const path = settings.value?.themeBackgroundImage;
    if (!path) {
      replaceBackgroundBlob([], "image/jpeg", undefined);
      return;
    }
    try {
      const data = await loadThemeBackground(path);
      replaceBackgroundBlob(data.bytes, data.mimeType, data.luminance);
    } catch {
      replaceBackgroundBlob([], "image/jpeg", undefined);
    }
  }

  /** Revokes the previous object URL so repeated previews do not leak image memory. */
  function replaceBackgroundBlob(bytes: number[], mimeType: string, luminance?: number): void {
    if (backgroundUrl.value) URL.revokeObjectURL(backgroundUrl.value);
    backgroundUrl.value = bytes.length
      ? URL.createObjectURL(new Blob([Uint8Array.from(bytes)], { type: mimeType }))
      : "";
    backgroundLuminance.value = luminance;
  }

  /** Clears an unsaved background selection immediately while leaving cached files recoverable. */
  function clearBackgroundPreview(): void {
    replaceBackgroundBlob([], "image/jpeg", undefined);
  }

  /** Persists a caller-edited immutable configuration snapshot. */
  async function save(next: AppConfig): Promise<void> {
    loading.value = true;
    error.value = "";
    try {
      settings.value = await updateSettings(next);
    } catch (cause) {
      error.value = String(cause);
      throw cause;
    } finally {
      loading.value = false;
    }
  }

  return { settings, loading, error, backgroundUrl, backgroundLuminance, load, save, importBackground, reloadBackground, clearBackgroundPreview };
});
