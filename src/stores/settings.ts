import { defineStore } from "pinia";
import { ref } from "vue";

import { getSettings, updateSettings } from "../api/settings";
import type { AppConfig } from "../models/settings";

export const useSettingsStore = defineStore("settings", () => {
  const settings = ref<AppConfig>();
  const loading = ref(false);
  const error = ref("");

  /** Loads the single shared configuration instead of letting pages read files independently. */
  async function load(): Promise<void> {
    loading.value = true;
    error.value = "";
    try {
      settings.value = await getSettings();
    } catch (cause) {
      error.value = String(cause);
    } finally {
      loading.value = false;
    }
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

  return { settings, loading, error, load, save };
});
