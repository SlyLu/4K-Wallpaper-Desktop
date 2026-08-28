<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";

import { useMonitorStore } from "../stores/monitor";
import { useWallpaperStore } from "../stores/wallpaper";
import type { WallpaperRecord } from "../models/wallpaper";
import { addCollectionWallpapers, removeCollectionWallpapers } from "../api/collections";

const props = defineProps<{
  emptyText?: string;
  management?: boolean;
  bulkMode?: "dislike" | "index" | "gallery" | "collection-add" | "collection-remove";
  collectionId?: number;
}>();
const wallpaperStore = useWallpaperStore();
const monitorStore = useMonitorStore();
const actionMessage = ref("");
const jumpPage = ref(1);
const bulkActive = ref(false);
const currentPageBulkSelected = computed(() =>
  wallpaperStore.wallpapers.length > 0
  && wallpaperStore.wallpapers.every((wallpaper) => Boolean(wallpaperStore.bulkSelections[wallpaper.id])),
);
const bulkActionLabel = computed(() => ({
  gallery: "从图库移除",
  dislike: "标记不喜欢",
  index: "移除索引",
  "collection-add": "加入集合",
  "collection-remove": "移出集合",
}[props.bulkMode ?? "index"]));
const bulkEntryLabel = computed(() => ({
  gallery: "批量管理图库",
  dislike: "批量标记不喜欢",
  index: "批量管理索引",
  "collection-add": "批量加入集合",
  "collection-remove": "批量移出集合",
}[props.bulkMode ?? "index"]));

/** Translates persisted lifecycle state into a concise card label. */
function availabilityLabel(wallpaper: WallpaperRecord): string {
  return {
    remote: "在线元数据",
    available: wallpaper.storageKind === "user_source" ? "本地源文件" : "已下载",
    temporarily_unavailable: "暂不可用",
    missing: "文件已缺失",
  }[wallpaper.fileAvailability];
}

watch(() => wallpaperStore.page, (page) => { jumpPage.value = page; }, { immediate: true });

/** Applies a card through the Rust Core to the current primary display. */
async function quickApply(wallpaperId: number): Promise<void> {
  const monitorId = monitorStore.primaryMonitor?.systemMonitorId;
  if (!monitorId) {
    actionMessage.value = "没有可用的主显示器";
    return;
  }
  actionMessage.value = "正在下载并设置…";
  try {
    await wallpaperStore.apply(wallpaperId, monitorId, "fill");
    actionMessage.value = "已设置到主显示器";
  } catch (cause) {
    actionMessage.value = String(cause);
  }
}

/** Removes only the gallery index or application-owned cache after explicit confirmation. */
async function removeFromGallery(wallpaper: WallpaperRecord): Promise<void> {
  const detail = wallpaper.provider === "local" ? "不会删除磁盘中的原始文件。" : "在线元数据仍会保留，可再次下载。";
  if (!window.confirm(`确定从图库移除“${wallpaper.name}”吗？${detail}`)) return;
  try {
    await wallpaperStore.removeFromLibrary(wallpaper);
    actionMessage.value = "已从图库移除";
  } catch (cause) {
    actionMessage.value = String(cause);
  }
}

/** Hides one disliked discovery card and keeps it excluded from later provider refreshes. */
async function dislikeOne(wallpaper: WallpaperRecord): Promise<void> {
  if (!window.confirm(`将“${wallpaper.name}”标记为不喜欢并从资源列表移除？`)) return;
  await wallpaperStore.blacklist(wallpaper.id);
  actionMessage.value = "已标记为不喜欢，后续刷新不会重新展示";
}

/** Copies the provider page URL for sharing without introducing a server-side share service. */
async function shareLink(wallpaper: WallpaperRecord): Promise<void> {
  const link = wallpaper.sourcePageUrl ?? wallpaper.originalUrl;
  if (!link) {
    actionMessage.value = "这张本地图片没有可分享的在线链接";
    return;
  }
  try {
    await navigator.clipboard.writeText(link);
    actionMessage.value = "资源链接已复制";
  } catch (cause) {
    actionMessage.value = `复制链接失败：${String(cause)}`;
  }
}

/** Changes the metadata page and returns the viewport to the first visible card. */
async function changePage(targetPage: number): Promise<void> {
  await wallpaperStore.goToPage(targetPage);
  document.querySelector(".wallpaper-grid")?.scrollIntoView({ behavior: "smooth", block: "start" });
}

/** Executes one explicit batch after explaining whether files or only indexes are affected. */
async function executeBulk(): Promise<void> {
  if (!props.bulkMode || !wallpaperStore.bulkSelectedCount) return;
  if (props.bulkMode === "collection-add" || props.bulkMode === "collection-remove") {
    if (!props.collectionId) return;
    const ids = Object.keys(wallpaperStore.bulkSelections).map(Number);
    const changed = props.bulkMode === "collection-add"
      ? await addCollectionWallpapers(props.collectionId, ids)
      : await removeCollectionWallpapers(props.collectionId, ids);
    wallpaperStore.clearBulkSelected();
    if (props.bulkMode === "collection-remove") await wallpaperStore.queryCollection(props.collectionId);
    actionMessage.value = `已${props.bulkMode === "collection-add" ? "加入" : "移出"} ${changed} 张壁纸`;
    bulkActive.value = false;
    return;
  }
  const explanation = props.bulkMode === "gallery"
    ? "在线下载会删除应用缓存；本地导入只从图库隐藏，不删除磁盘原文件。"
    : "所选资源将从列表隐藏，后续刷新或扫描不会重新展示；本地原始文件不会被删除。";
  if (!window.confirm(`确定${bulkActionLabel.value}所选 ${wallpaperStore.bulkSelectedCount} 张壁纸吗？${explanation}`)) return;
  const result = await wallpaperStore.removeBulkSelected(props.bulkMode);
  actionMessage.value = `已处理 ${result.removed} 张${result.failed ? `，${result.failed} 张处理失败` : ""}`;
  bulkActive.value = false;
}

/** Makes batch selection an explicit mode so it cannot be confused with rotation selection. */
function setBulkActive(active: boolean): void {
  bulkActive.value = active;
  if (!active) wallpaperStore.clearBulkSelected();
}

/** Card clicks select in batch mode and open details during ordinary browsing. */
function handleCardClick(wallpaper: WallpaperRecord): void {
  if (bulkActive.value) wallpaperStore.toggleBulkSelected(wallpaper);
  else wallpaperStore.activeWallpaper = wallpaper;
}

onBeforeUnmount(() => wallpaperStore.clearBulkSelected());
</script>

<template>
  <p v-if="actionMessage" class="inline-status">{{ actionMessage }}</p>
  <p v-if="wallpaperStore.error" class="message error">{{ wallpaperStore.error }}</p>
  <section v-if="props.bulkMode && !bulkActive" class="bulk-entry">
    <div><strong>{{ bulkEntryLabel }}</strong><span>进入后可跨页选择，轮换勾选不会受到影响。</span></div>
    <button class="secondary" @click="setBulkActive(true)">进入批量管理</button>
  </section>
  <section v-if="props.bulkMode && bulkActive" class="bulk-toolbar">
    <div><strong>批量管理</strong><span>已跨页选择 {{ wallpaperStore.bulkSelectedCount }} 张</span></div>
    <div class="actions"><button class="secondary" @click="wallpaperStore.setCurrentPageBulkSelected(!currentPageBulkSelected)">{{ currentPageBulkSelected ? "取消本页" : "选择本页" }}</button><button v-if="wallpaperStore.bulkSelectedCount" class="secondary" @click="wallpaperStore.clearBulkSelected">清空选择</button><button class="danger" :disabled="!wallpaperStore.bulkSelectedCount" @click="executeBulk">{{ bulkActionLabel }}</button><button class="secondary" @click="setBulkActive(false)">退出批量管理</button></div>
  </section>
  <div v-if="wallpaperStore.loading" class="loading-grid">
    <span v-for="index in 8" :key="index"></span>
  </div>
  <div v-else-if="wallpaperStore.wallpapers.length" class="wallpaper-grid">
    <article
      v-for="wallpaper in wallpaperStore.wallpapers"
      :key="wallpaper.id"
      class="wallpaper-card"
      :class="{ selected: !bulkActive && wallpaperStore.selectedIds.includes(wallpaper.id), 'batch-mode': bulkActive, 'batch-selected': bulkActive && wallpaperStore.bulkSelections[wallpaper.id] }"
      tabindex="0"
      @click="handleCardClick(wallpaper)"
      @keydown.enter="handleCardClick(wallpaper)"
    >
      <img :src="wallpaperStore.thumbnailFor(wallpaper)" :alt="wallpaper.name" loading="lazy" />
      <div class="card-shade"></div>
      <button
        v-if="!bulkActive"
        class="select-button"
        :aria-label="wallpaperStore.selectedIds.includes(wallpaper.id) ? '从轮换池取消' : '加入轮换池'"
        :title="wallpaperStore.selectedIds.includes(wallpaper.id) ? '取消自动切换选择' : '加入自动切换选择池'"
        @click.stop="wallpaperStore.toggleSelected(wallpaper.id)"
      >{{ wallpaperStore.selectedIds.includes(wallpaper.id) ? "✓ 已加入轮换" : "+ 加入轮换" }}</button>
      <button
        v-if="props.bulkMode && bulkActive"
        class="bulk-select-button"
        :class="{ active: wallpaperStore.bulkSelections[wallpaper.id] }"
        :aria-label="wallpaperStore.bulkSelections[wallpaper.id] ? '取消批量选择' : '加入批量选择'"
        @click.stop="wallpaperStore.toggleBulkSelected(wallpaper)"
      >{{ wallpaperStore.bulkSelections[wallpaper.id] ? "✓ 已选择" : "批量选择" }}</button>
      <button
        v-if="!bulkActive"
        class="heart-button"
        :class="{ active: wallpaper.favorite }"
        :aria-label="wallpaper.favorite ? '取消收藏' : '收藏'"
        @click.stop="wallpaperStore.toggleFavorite(wallpaper)"
      >♥</button>
      <div v-if="!bulkActive" class="card-actions">
        <button @click.stop="quickApply(wallpaper.id)">设为壁纸</button>
        <button v-if="!props.management" class="glass" @click.stop="wallpaperStore.activeWallpaper = wallpaper">查看原图</button>
        <button v-if="props.bulkMode === 'dislike'" class="danger" @click.stop="dislikeOne(wallpaper)">不喜欢</button>
        <template v-if="props.management">
          <button v-if="wallpaper.sourcePageUrl || wallpaper.originalUrl" class="glass" @click.stop="shareLink(wallpaper)">复制链接</button>
          <button class="danger" @click.stop="removeFromGallery(wallpaper)">从图库移除</button>
        </template>
      </div>
      <div class="card-meta">
        <strong>{{ wallpaper.name }}</strong>
        <span>{{ wallpaper.width }} × {{ wallpaper.height }} · {{ wallpaper.provider }} · {{ availabilityLabel(wallpaper) }}<template v-if="wallpaper.fileCopyCount > 1"> · {{ wallpaper.fileCopyCount }} 个副本</template></span>
      </div>
    </article>
  </div>
  <div v-else class="empty-state">
    <strong>暂时没有壁纸</strong>
    <p>{{ emptyText ?? "尝试调整筛选条件或刷新在线资源。" }}</p>
  </div>
  <nav v-if="!wallpaperStore.loading && wallpaperStore.wallpapers.length && wallpaperStore.total > wallpaperStore.pageSize" class="catalog-pagination" aria-label="壁纸资源分页">
    <button class="secondary" :disabled="wallpaperStore.page <= 1" @click="changePage(1)">首页</button>
    <button class="secondary" :disabled="wallpaperStore.page <= 1" @click="changePage(wallpaperStore.page - 1)">上一页</button>
    <span>第 {{ wallpaperStore.page }} / {{ Math.ceil(wallpaperStore.total / wallpaperStore.pageSize) }} 页 · 当前 {{ (wallpaperStore.page - 1) * wallpaperStore.pageSize + 1 }}–{{ Math.min(wallpaperStore.total, wallpaperStore.page * wallpaperStore.pageSize) }} / 共 {{ wallpaperStore.total }} 张</span>
    <form class="page-jump" @submit.prevent="changePage(jumpPage)"><label>跳至 <input v-model.number="jumpPage" type="number" min="1" :max="Math.ceil(wallpaperStore.total / wallpaperStore.pageSize)" /> 页</label><button class="secondary" type="submit">跳转</button></form>
    <button class="secondary" :disabled="wallpaperStore.page * wallpaperStore.pageSize >= wallpaperStore.total" @click="changePage(wallpaperStore.page + 1)">下一页</button>
    <button class="secondary" :disabled="wallpaperStore.page * wallpaperStore.pageSize >= wallpaperStore.total" @click="changePage(Math.ceil(wallpaperStore.total / wallpaperStore.pageSize))">尾页</button>
  </nav>
</template>
