<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from "vue";

import type { FitMode } from "../models/image";
import { useMonitorStore } from "../stores/monitor";
import { useWallpaperStore } from "../stores/wallpaper";

const wallpaperStore = useWallpaperStore();
const monitorStore = useMonitorStore();
const originalUrl = ref("");
const loading = ref(false);
const error = ref("");
const monitorId = ref("");
const fitMode = ref<FitMode>("fill");
const success = ref("");
const requestedWallpaperId = ref<number>();

/** Clicking a card intentionally downloads, validates, and displays its original image. */
watch(
  () => wallpaperStore.activeWallpaper,
  async (wallpaper) => {
    if (wallpaper && requestedWallpaperId.value === wallpaper.id) return;
    releaseOriginal();
    if (!wallpaper) return;
    requestedWallpaperId.value = wallpaper.id;
    monitorId.value = monitorStore.primaryMonitor?.systemMonitorId ?? "";
    loading.value = true;
    try {
      const result = await wallpaperStore.downloadOriginal(wallpaper.id);
      wallpaperStore.activeWallpaper = result.wallpaper;
      const blob = new Blob([result.bytes], { type: result.wallpaper.mimeType ?? "image/jpeg" });
      originalUrl.value = URL.createObjectURL(blob);
    } catch (cause) {
      error.value = String(cause);
    } finally {
      loading.value = false;
    }
  },
);

/** Applies the open original to the explicitly selected display. */
async function apply(): Promise<void> {
  const wallpaper = wallpaperStore.activeWallpaper;
  if (!wallpaper || !monitorId.value) return;
  loading.value = true;
  error.value = "";
  try {
    await wallpaperStore.apply(wallpaper.id, monitorId.value, fitMode.value);
    success.value = "壁纸已应用到选中显示器";
  } catch (cause) {
    error.value = String(cause);
  } finally {
    loading.value = false;
  }
}

/** Removes the downloaded remote original while preserving favorites and local files. */
async function deleteCache(): Promise<void> {
  const wallpaper = wallpaperStore.activeWallpaper;
  if (!wallpaper) return;
  try {
    await wallpaperStore.deleteCache(wallpaper.id);
    releaseOriginal();
    requestedWallpaperId.value = undefined;
    success.value = "本地原图缓存已删除";
  } catch (cause) {
    error.value = String(cause);
  }
}

/** Closes the overlay and immediately releases multi-megabyte Blob memory. */
function close(): void {
  releaseOriginal();
  requestedWallpaperId.value = undefined;
  wallpaperStore.activeWallpaper = undefined;
}

function releaseOriginal(): void {
  if (originalUrl.value) URL.revokeObjectURL(originalUrl.value);
  originalUrl.value = "";
  error.value = "";
  success.value = "";
}

onBeforeUnmount(releaseOriginal);
</script>

<template>
  <div v-if="wallpaperStore.activeWallpaper" class="detail-backdrop" @click="close">
    <section class="detail-modal" role="dialog" aria-modal="true" @click.stop>
      <button class="detail-close" aria-label="关闭原图" @click="close">×</button>
      <div class="detail-preview">
        <img v-if="originalUrl" :src="originalUrl" :alt="wallpaperStore.activeWallpaper.name" />
        <div v-else class="detail-loading">
          <span class="spinner"></span>
          {{ loading ? "正在下载并校验高清原图…" : "原图暂不可用" }}
        </div>
      </div>
      <aside class="detail-info">
        <p class="eyebrow">{{ wallpaperStore.activeWallpaper.provider }}</p>
        <h2>{{ wallpaperStore.activeWallpaper.name }}</h2>
        <p>{{ wallpaperStore.activeWallpaper.width }} × {{ wallpaperStore.activeWallpaper.height }}</p>
        <p>{{ wallpaperStore.activeWallpaper.fileSize ? `${(wallpaperStore.activeWallpaper.fileSize / 1048576).toFixed(1)} MB` : "文件大小未知" }} · {{ wallpaperStore.activeWallpaper.mimeType ?? "image" }}</p>
        <div class="tag-list">
          <span v-for="tag in wallpaperStore.activeWallpaper.tags" :key="tag">{{ tag }}</span>
        </div>
        <label>目标显示器<select v-model="monitorId">
          <option v-for="monitor in monitorStore.monitors" :key="monitor.systemMonitorId" :value="monitor.systemMonitorId">{{ monitor.name }} · {{ monitor.width }}×{{ monitor.height }}</option>
        </select></label>
        <label>适配方式<select v-model="fitMode">
          <option value="fill">Fill</option><option value="fit">Fit</option><option value="center">Center</option><option value="stretch">Stretch</option>
        </select></label>
        <p v-if="error" class="message error">{{ error }}</p>
        <p v-if="success" class="message">{{ success }}</p>
        <div class="detail-actions">
          <button :disabled="loading || !originalUrl" @click="apply">设为壁纸</button>
          <button class="secondary" @click="wallpaperStore.toggleFavorite(wallpaperStore.activeWallpaper!)">{{ wallpaperStore.activeWallpaper.favorite ? "取消收藏" : "收藏" }}</button>
          <button class="danger" @click="wallpaperStore.blacklist(wallpaperStore.activeWallpaper!.id)">不喜欢</button>
          <button v-if="wallpaperStore.activeWallpaper.provider !== 'local' && wallpaperStore.activeWallpaper.localPath" class="secondary" @click="deleteCache">删除本地缓存</button>
        </div>
      </aside>
    </section>
  </div>
</template>
