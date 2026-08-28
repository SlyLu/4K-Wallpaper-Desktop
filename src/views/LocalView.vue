<script setup lang="ts">
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { onBeforeUnmount, onMounted, ref } from "vue";

import {
  importLocalPaths,
  listDuplicateFileGroups,
  pruneMissingLocalWallpapers,
  scanLocalDirectory,
} from "../api/catalog";
import WallpaperGrid from "../components/WallpaperGrid.vue";
import type { CatalogQuery, DuplicateFileGroup, WallpaperRecord } from "../models/wallpaper";
import { useSettingsStore } from "../stores/settings";
import { useWallpaperStore } from "../stores/wallpaper";

type GallerySource = "all" | "local" | "online";
type GalleryCategory = "all" | "nature" | "anime" | "people";

const wallpaperStore = useWallpaperStore();
const settingsStore = useSettingsStore();
const source = ref<GallerySource>("all");
const nameFilter = ref("");
const category = ref<GalleryCategory>("all");
const availability = ref<"all" | WallpaperRecord["fileAvailability"]>("all");
const duplicateGroups = ref<DuplicateFileGroup[]>([]);
const operationMessage = ref("");
const scanning = ref(false);
const refreshing = ref(false);
const dragActive = ref(false);
let unlistenDragDrop: (() => void) | undefined;

/** Builds a device-gallery query that keeps local imports and online downloads distinct. */
function loadGallery(): Promise<void> {
  const query: CatalogQuery = {
    fileBacked: true,
    name: nameFilter.value.trim() || undefined,
    category: category.value,
    pageSize: 100,
  };
  if (source.value === "local") query.storageKind = "user_source";
  if (source.value === "online") query.storageKind = "managed_download";
  if (availability.value !== "all") query.fileAvailability = availability.value;
  return wallpaperStore.query(query);
}

/** Refreshes duplicate-copy review data independently from the paginated gallery. */
async function loadDuplicates(): Promise<void> {
  duplicateGroups.value = await listDuplicateFileGroups();
}

/** Opens the native directory picker and indexes supported files without moving them. */
async function addDirectory(): Promise<void> {
  const selected = await open({ directory: true, multiple: false, title: "选择壁纸目录" });
  if (!selected) return;
  scanning.value = true;
  operationMessage.value = "正在扫描、校验并生成缩略图…";
  try {
    const count = await scanLocalDirectory(selected);
    await Promise.all([settingsStore.load(), loadGallery(), loadDuplicates()]);
    operationMessage.value = `扫描完成，已索引 ${count} 张图片；原始文件保持原位`;
  } catch (cause) {
    operationMessage.value = String(cause);
  } finally {
    scanning.value = false;
  }
}

/** Reindexes tracked roots and removes entries for files deleted outside the app. */
async function refreshGallery(): Promise<void> {
  refreshing.value = true;
  operationMessage.value = "正在校对文件和图库索引…";
  let indexed = 0;
  let failed = 0;
  try {
    await settingsStore.load();
    for (const directory of settingsStore.settings?.localDirectories ?? []) {
      try {
        indexed += await scanLocalDirectory(directory);
      } catch {
        failed += 1;
      }
    }
    const removed = await pruneMissingLocalWallpapers();
    await Promise.all([loadGallery(), loadDuplicates()]);
    operationMessage.value = `刷新完成：检查 ${indexed} 张，更新 ${removed} 条文件状态${failed ? `，${failed} 个目录暂不可访问` : ""}`;
  } catch (cause) {
    operationMessage.value = String(cause);
  } finally {
    refreshing.value = false;
  }
}

/** Imports files or folders dropped from Explorer/Finder through LocalProvider validation. */
async function importDropped(paths: string[]): Promise<void> {
  if (!paths.length || scanning.value || refreshing.value) return;
  scanning.value = true;
  operationMessage.value = "正在导入拖入的图片…";
  try {
    const count = await importLocalPaths(paths);
    await Promise.all([settingsStore.load(), loadGallery(), loadDuplicates()]);
    operationMessage.value = `已导入 ${count} 张有效图片`;
  } catch (cause) {
    operationMessage.value = String(cause);
  } finally {
    scanning.value = false;
  }
}

onMounted(async () => {
  await Promise.all([settingsStore.load(), loadGallery(), loadDuplicates()]);
  unlistenDragDrop = await getCurrentWebview().onDragDropEvent(({ payload }) => {
    if (payload.type === "enter") dragActive.value = true;
    if (payload.type === "leave") dragActive.value = false;
    if (payload.type === "drop") {
      dragActive.value = false;
      void importDropped(payload.paths);
    }
  });
});

onBeforeUnmount(() => unlistenDragDrop?.());
</script>

<template>
  <header class="page-header">
    <div><p class="eyebrow">WALLPAPER LIBRARY</p><h1>图库</h1><p>统一管理本地导入和在线下载的高清原图。</p></div>
    <div class="actions"><button class="secondary" :disabled="scanning || refreshing" @click="refreshGallery">{{ refreshing ? "刷新中…" : "刷新图库" }}</button><button :disabled="scanning || refreshing" @click="addDirectory">{{ scanning ? "处理中…" : "扫描添加" }}</button></div>
  </header>

  <section class="gallery-toolbar">
    <div class="feed-tabs" aria-label="图库来源">
      <button :class="{ active: source === 'all' }" @click="source = 'all'; loadGallery()">全部图库</button>
      <button :class="{ active: source === 'local' }" @click="source = 'local'; loadGallery()">本地导入</button>
      <button :class="{ active: source === 'online' }" @click="source = 'online'; loadGallery()">在线下载</button>
    </div>
    <form class="gallery-filters" @submit.prevent="loadGallery">
      <input v-model="nameFilter" placeholder="按图片名称筛选" />
      <select v-model="category"><option value="all">全部分类</option><option value="nature">自然</option><option value="anime">动漫</option><option value="people">人物</option></select>
      <select v-model="availability"><option value="all">全部文件状态</option><option value="available">本机可用</option><option value="temporarily_unavailable">暂不可用</option><option value="missing">已缺失</option></select>
      <button type="submit" class="secondary">筛选</button>
    </form>
  </section>

  <div class="drop-zone" :class="{ active: dragActive }">
    <strong>{{ dragActive ? "松开即可导入" : "拖入图片或文件夹" }}</strong>
    <span>支持 JPG、JPEG、PNG、WebP；导入只建立索引，不移动原文件。</span>
  </div>
  <div v-if="settingsStore.settings?.localDirectories.length" class="directory-chips"><span v-for="path in settingsStore.settings.localDirectories" :key="path">{{ path }}</span></div>
  <p v-if="operationMessage" class="inline-status">{{ operationMessage }}</p>
  <div class="section-title"><h2>{{ source === 'local' ? '本地导入' : source === 'online' ? '在线下载' : '全部图库' }}</h2><p>{{ wallpaperStore.total }} 张</p></div>
  <WallpaperGrid management bulk-mode="gallery" empty-text="拖入图片、扫描目录，或先从在线资源下载高清原图。" />
  <section v-if="duplicateGroups.length" class="duplicate-panel">
    <div class="section-title"><h2>重复文件</h2><p>{{ duplicateGroups.length }} 组</p></div>
    <details v-for="group in duplicateGroups" :key="group.contentHash">
      <summary>{{ group.copies.length }} 个相同内容副本 · {{ group.contentHash.slice(0, 12) }}</summary>
      <ul><li v-for="copy in group.copies" :key="copy.path"><span>{{ copy.path }}</span><small>{{ copy.storageKind }} · {{ copy.availability }}</small></li></ul>
    </details>
  </section>
</template>
