<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";

import WallpaperGrid from "../components/WallpaperGrid.vue";
import type { CatalogQuery, ProviderQuery } from "../models/wallpaper";
import { useWallpaperStore } from "../stores/wallpaper";

const wallpaperStore = useWallpaperStore();
const filters = reactive({ keyword: "", category: "all", resolution: "0", provider: "all", favorite: "all" });
const onlineMessage = ref("");
const searching = ref(false);
const canSearchOnline = computed(
  () => filters.provider !== "local" && filters.category !== "local" && filters.favorite !== "yes",
);

/** Builds the local metadata filters once so fallback results use identical constraints. */
function catalogQuery(): CatalogQuery {
  const minWidth = Number(filters.resolution);
  return {
    keyword: filters.keyword || undefined,
    category: filters.category as "all" | "nature" | "anime" | "people" | "local",
    provider: filters.provider as "all" | "wallhaven" | "local",
    favorite: filters.favorite === "all" ? undefined : filters.favorite === "yes",
    minWidth: minWidth || undefined,
    minHeight: minWidth >= 3840 ? 2160 : undefined,
    pageSize: 60,
  };
}

/** Converts compatible local filters into the bounded SFW Wallhaven query contract. */
function providerQuery(): ProviderQuery {
  return {
    keyword: filters.keyword.trim() || undefined,
    category: filters.category as "all" | "nature" | "anime" | "people",
    minWidth: Number(filters.resolution) || 3840,
    minHeight: 2160,
    aspectRatio: "16:9",
    page: 1,
    pageSize: 24,
    sort: "latest",
    safety: "sfw",
  };
}

/** Searches SQLite first and automatically supplements an empty result from Wallhaven. */
async function search(): Promise<void> {
  if (searching.value) return;
  searching.value = true;
  onlineMessage.value = "";
  try {
    await wallpaperStore.query(catalogQuery());
    if (wallpaperStore.error || wallpaperStore.total > 0 || !canSearchOnline.value) return;

    onlineMessage.value = "本地没有匹配项，正在搜索 Wallhaven 在线资源库…";
    try {
      const imported = await wallpaperStore.syncOnline(providerQuery());
      onlineMessage.value = wallpaperStore.total > 0
        ? `已从在线资源库拉取 ${imported} 条元数据，找到 ${wallpaperStore.total} 张匹配壁纸`
        : "本地和在线资源库均未找到匹配壁纸";
    } catch {
      onlineMessage.value = "在线搜索失败，已保留本地搜索结果";
    }
  } finally {
    searching.value = false;
  }
}

/** Forces a fresh online metadata query even when matching local results already exist. */
async function searchOnline(): Promise<void> {
  if (searching.value || !canSearchOnline.value) return;
  searching.value = true;
  onlineMessage.value = "正在搜索 Wallhaven 在线资源库…";
  try {
    // Establish the current local filters before syncOnline refreshes its last catalog query.
    await wallpaperStore.query(catalogQuery());
    const imported = await wallpaperStore.syncOnline(providerQuery());
    onlineMessage.value = `在线资源库已拉取 ${imported} 条元数据，当前找到 ${wallpaperStore.total} 张`;
  } catch {
    onlineMessage.value = "在线搜索失败，已保留本地搜索结果";
  } finally {
    searching.value = false;
  }
}

onMounted(() => void search());
</script>

<template>
  <header class="page-header"><div><p class="eyebrow">METADATA SEARCH</p><h1>搜索壁纸</h1><p>优先搜索本地元数据，没有结果时自动从在线资源库拉取。</p></div></header>
  <form class="search-panel" @submit.prevent="search">
    <input v-model="filters.keyword" autofocus placeholder="搜索 mountain、雪山、sunset、anime…" />
    <select v-model="filters.category"><option value="all">全部分类</option><option value="nature">自然</option><option value="anime">动漫</option><option value="people">人物</option><option value="local">本地</option></select>
    <select v-model="filters.resolution"><option value="0">全部分辨率</option><option value="3840">≥ 4K</option><option value="5120">≥ 5K</option><option value="7680">≥ 8K</option></select>
    <select v-model="filters.provider"><option value="all">全部来源</option><option value="wallhaven">Wallhaven</option><option value="local">Local</option></select>
    <select v-model="filters.favorite"><option value="all">全部收藏状态</option><option value="yes">仅收藏</option><option value="no">未收藏</option></select>
    <button type="submit" :disabled="searching">{{ searching ? "搜索中…" : "搜索" }}</button><button type="button" class="secondary" :disabled="searching || !canSearchOnline" @click="searchOnline">联网搜索</button>
  </form>
  <p v-if="onlineMessage" class="inline-status">{{ onlineMessage }}</p>
  <div class="section-title"><h2>搜索结果</h2><p>{{ wallpaperStore.total }} 张</p></div>
  <WallpaperGrid bulk-mode="index" empty-text="本地及在线资源库均未找到匹配项。" />
</template>
