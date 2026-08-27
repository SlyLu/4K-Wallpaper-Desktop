import { defineStore } from "pinia";

import { getMonitors } from "../api/platform";
import type { MonitorInfo } from "../models/monitor";

export const useMonitorStore = defineStore("monitor", {
  state: () => ({
    monitors: [] as MonitorInfo[],
    loading: false,
    error: "",
  }),
  getters: {
    primaryMonitor: (state) => state.monitors.find((monitor) => monitor.primary),
    hasMultipleMonitors: (state) => state.monitors.length > 1,
  },
  actions: {
    /** Refreshes monitor state while keeping invoke errors visible to the validation UI. */
    async refresh(): Promise<void> {
      this.loading = true;
      this.error = "";
      try {
        this.monitors = await getMonitors();
      } catch (error) {
        this.error = String(error);
      } finally {
        this.loading = false;
      }
    },
  },
});
