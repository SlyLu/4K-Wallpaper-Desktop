<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { RouterLink, RouterView, useRouter } from "vue-router";

import { syncCatalogIfDue } from "../api/catalog";
import { useMonitorStore } from "../stores/monitor";
import { useSchedulerStore } from "../stores/scheduler";
import { useSettingsStore } from "../stores/settings";
import { applyTheme } from "../utils/theme";
import WallpaperDetail from "./WallpaperDetail.vue";

const monitorStore = useMonitorStore();
const schedulerStore = useSchedulerStore();
const settingsStore = useSettingsStore();
const router = useRouter();
const nextMessage = ref("");
let monitorRefreshTimer: ReturnType<typeof setInterval> | undefined;
const systemTheme = window.matchMedia("(prefers-color-scheme: light)");

/** Reapplies system-following themes when Windows or macOS appearance changes. */
function handleSystemThemeChange(): void {
  if (settingsStore.settings?.themeMode === "system") applyTheme(settingsStore.settings);
}

watch(
  () => settingsStore.settings,
  (settings) => {
    if (settings) applyTheme(settings);
  },
  { deep: true },
);

const navigation = [
  ["/discover", "发现", "D"],
  ["/categories", "分类", "C"],
  ["/search", "搜索", "S"],
  ["/favorites", "收藏", "F"],
  ["/local", "图库", "L"],
  ["/displays", "显示器", "M"],
  ["/settings", "设置", "⚙"],
] as const;

/** Provides one obvious global Next entry and guides unconfigured users to display settings. */
async function nextWallpaper(): Promise<void> {
  const primaryId = monitorStore.primaryMonitor?.systemMonitorId;
  const schedule = schedulerStore.schedules.find(
    (item) => item.enabled && item.systemMonitorId === primaryId,
  ) ?? schedulerStore.schedules.find((item) => item.enabled);
  if (!schedule) {
    nextMessage.value = "请先配置自动切换";
    await router.push("/displays");
    return;
  }
  try {
    await schedulerStore.next(schedule.systemMonitorId);
    nextMessage.value = "已请求切换下一张";
  } catch {
    nextMessage.value = schedulerStore.error;
  }
}

/** Initializes shared stores once so individual pages never duplicate platform calls. */
onMounted(async () => {
  systemTheme.addEventListener("change", handleSystemThemeChange);
  await Promise.all([monitorStore.refresh(), schedulerStore.refresh(), settingsStore.load()]);
  // The main UI is already visible; provider failures never block or replace local browsing.
  void syncCatalogIfDue().catch(() => undefined);
  // Periodic snapshots cover display hot-plug and resolution/primary-display changes.
  monitorRefreshTimer = setInterval(() => void monitorStore.refresh(), 30_000);
});

onBeforeUnmount(() => {
  if (monitorRefreshTimer) clearInterval(monitorRefreshTimer);
  systemTheme.removeEventListener("change", handleSystemThemeChange);
});
</script>

<template>
  <div class="app-layout">
    <aside class="sidebar">
      <RouterLink class="brand" to="/discover" aria-label="4K Wallpaper Desktop 首页">
        <span class="brand-mark">4K</span>
        <span><strong>Wallpaper</strong><small>Desktop</small></span>
      </RouterLink>
      <nav class="sidebar-nav" aria-label="主导航">
        <RouterLink v-for="item in navigation" :key="item[0]" :to="item[0]">
          <span class="nav-icon">{{ item[2] }}</span><span>{{ item[1] }}</span>
        </RouterLink>
      </nav>
      <button class="sidebar-next" @click="nextWallpaper"><span>↻</span><strong>下一张壁纸</strong></button>
      <small v-if="nextMessage" class="sidebar-next-message">{{ nextMessage }}</small>
      <div class="sidebar-status">
        <span class="status-dot"></span>
        <div>
          <strong>{{ monitorStore.monitors.length }} 块显示器</strong>
          <small>{{ schedulerStore.schedules.some((item) => item.enabled && !item.paused) ? "自动切换运行中" : "本地核心就绪" }}</small>
        </div>
      </div>
    </aside>
    <main class="app-content">
      <RouterView />
    </main>
    <WallpaperDetail />
  </div>
</template>
