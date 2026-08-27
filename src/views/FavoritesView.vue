<script setup lang="ts">
import { onMounted } from "vue";
import WallpaperGrid from "../components/WallpaperGrid.vue";
import { useWallpaperStore } from "../stores/wallpaper";
const wallpaperStore = useWallpaperStore();
onMounted(() => wallpaperStore.query({ favorite: true, pageSize: 60 }));
</script>
<template><header class="page-header"><div><p class="eyebrow">FAVORITES</p><h1>我的收藏</h1><p>收藏原图受缓存清理保护，并可加入任意显示器轮换池。</p></div></header><div class="section-title"><h2>已收藏</h2><p>{{ wallpaperStore.total }} 张</p></div><WallpaperGrid bulk-mode="index" empty-text="点击任意壁纸卡片上的爱心即可收藏。" /></template>
