<script setup lang="ts">
import { onMounted, ref } from "vue";

import WallpaperGrid from "../components/WallpaperGrid.vue";
import type { ProviderSort } from "../models/wallpaper";
import { useWallpaperStore } from "../stores/wallpaper";
import { errorMessage } from "../utils/error";

const wallpaperStore = useWallpaperStore();
const feed = ref<ProviderSort>("popular");
const refreshMessage = ref("");

/** Maps the three required Discover feeds to metadata sorting without loading originals. */
async function loadFeed(): Promise<void> {
  await wallpaperStore.query({ sort: feed.value === "random" ? "random" : "latest", pageSize: 30 });
}

/** Manually synchronizes one SFW 4K Wallhaven page, then renders persisted metadata. */
async function refreshOnline(): Promise<void> {
  refreshMessage.value = "";
  try {
    const count = await wallpaperStore.syncOnline({
      category: "all",
      minWidth: 3840,
      minHeight: 2160,
      aspectRatio: "16:9",
      page: 1,
      pageSize: 24,
      sort: feed.value,
      safety: "sfw",
    });
    refreshMessage.value = `已同步 ${count} 条在线元数据，原图仍按需下载`;
  } catch (cause) {
    refreshMessage.value = `在线资源暂不可用：${errorMessage(cause)}；本地资源仍可正常使用`;
  }
}

onMounted(loadFeed);
</script>

<template>
  <header class="page-header discover-hero">
    <div><p class="eyebrow">DISCOVER 4K</p><h1>为你的屏幕，找一张新风景</h1><p>浏览缩略图，点击时才下载高清原图。</p></div>
    <button :disabled="wallpaperStore.syncing" @click="refreshOnline">{{ wallpaperStore.syncing ? "同步中…" : "刷新在线资源" }}</button>
  </header>
  <div class="feed-tabs">
    <button v-for="item in ([['popular','推荐'],['latest','最新'],['random','随机']] as const)" :key="item[0]" :class="{ active: feed === item[0] }" @click="feed = item[0]; loadFeed()">{{ item[1] }}</button>
  </div>
  <div class="section-title"><div><h2>{{ feed === 'popular' ? '推荐壁纸' : feed === 'latest' ? '最新壁纸' : '随机发现' }}</h2><p>本机资源索引 {{ wallpaperStore.total }} 张 · 当前页 {{ wallpaperStore.wallpapers.length }} 张</p></div><span v-if="refreshMessage" class="inline-status">{{ refreshMessage }}</span></div>
  <WallpaperGrid bulk-mode="dislike" />
</template>
