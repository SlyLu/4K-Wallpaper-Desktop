<script setup lang="ts">
import { onMounted, ref } from "vue";

import WallpaperGrid from "../components/WallpaperGrid.vue";
import { useWallpaperStore } from "../stores/wallpaper";

const wallpaperStore = useWallpaperStore();
const category = ref<"all" | "nature" | "anime" | "games" | "people" | "local">("all");
const categories = [["all", "全部", "所有来源"], ["nature", "自然", "山川与城市"], ["anime", "动漫", "插画与动画"], ["games", "游戏", "Fanart 与截图"], ["people", "人物", "人物摄影"], ["local", "本地", "你的图库"]] as const;

/** Treats Local as device availability while keeping downloaded items' original categories. */
function loadCategory(): Promise<void> {
  return category.value === "local"
    ? wallpaperStore.query({ locallyAvailable: true, pageSize: 60 })
    : wallpaperStore.query({ category: category.value, pageSize: 60 });
}

onMounted(loadCategory);
</script>

<template>
  <header class="page-header"><div><p class="eyebrow">CATEGORIES</p><h1>按主题浏览</h1><p>统一分类不会暴露具体 Provider 的内部标记。</p></div></header>
  <div class="category-strip">
    <button v-for="item in categories" :key="item[0]" :class="{ active: category === item[0] }" @click="category = item[0]; loadCategory()"><strong>{{ item[1] }}</strong><small>{{ item[2] }}</small></button>
    <RouterLink class="category-link" to="/favorites"><strong>收藏</strong><small>已保护的原图</small></RouterLink>
  </div>
  <div class="section-title"><h2>{{ categories.find((item) => item[0] === category)?.[1] }}</h2><p>{{ wallpaperStore.total }} 张</p></div>
  <WallpaperGrid bulk-mode="index" />
</template>
