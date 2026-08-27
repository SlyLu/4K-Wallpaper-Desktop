<script setup lang="ts">
import { confirm, open } from "@tauri-apps/plugin-dialog";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import { removeLocalDirectory, scanLocalDirectory } from "../api/catalog";
import { clearCache, getCacheInfo } from "../api/cache";
import type { CacheInfo } from "../models/cache";
import type { AppConfig } from "../models/settings";
import { useSettingsStore } from "../stores/settings";
import { useWallpaperStore } from "../stores/wallpaper";
import { applyTheme, readableText } from "../utils/theme";

const settingsStore = useSettingsStore();
const wallpaperStore = useWallpaperStore();
const draft = ref<AppConfig>();
const message = ref("");
const cacheInfo = ref<CacheInfo>();
const systemPrefersLight = ref(window.matchMedia("(prefers-color-scheme: light)").matches);
const cacheLabel = computed(() => draft.value?.cacheLimitBytes === 0 ? "无限制" : `${Math.round(draft.value.cacheLimitBytes / 1073741824)} GB`);

/** Builds an honest preview from the selected mode and keeps its foreground readable. */
const themePreviewStyle = computed<Record<string, string>>(() => {
  const settings = draft.value;
  const usesLightDefaults = settings?.themeMode === "light" || (settings?.themeMode === "system" && systemPrefersLight.value);
  const palette = settings?.themeMode === "custom"
    ? { accent: settings.themeAccent, secondary: settings.themeSecondary, background: settings.themeBackground, surface: settings.themeSurface }
    : usesLightDefaults
      ? { accent: "#087f9c", secondary: "#377bd1", background: "#eef6fb", surface: "#ffffff" }
      : { accent: "#64e8f5", secondary: "#4eb2f4", background: "#07111d", surface: "#0a1b29" };
  return {
    "--preview-accent": palette.accent,
    "--preview-secondary": palette.secondary,
    "--preview-bg": palette.background,
    "--preview-surface": palette.surface,
    "--preview-text": readableText(palette.background),
  };
});

/** Converts Pinia's reactive proxy into an editable plain configuration object. */
function cloneSettings(): AppConfig | undefined {
  return settingsStore.settings
    ? JSON.parse(JSON.stringify(settingsStore.settings))
    : undefined;
}

/** Previews visual-only theme edits immediately; leaving restores the last persisted theme. */
watch(
  draft,
  (settings) => {
    if (settings) applyTheme(settings);
  },
  { deep: true },
);

/** System mode intentionally excludes user-selected background effects in V1. */
watch(
  () => draft.value?.themeMode,
  (mode) => {
    if (mode === "system" && draft.value) draft.value.themeEffect = "solid";
  },
);

/** Copies persisted settings so canceling edits never mutates the shared store. */
async function load(): Promise<void> {
  const [, info] = await Promise.all([settingsStore.load(), getCacheInfo()]);
  cacheInfo.value = info;
  draft.value = cloneSettings();
}

/** Requires explicit user confirmation before deleting removable application cache files. */
async function clearApplicationCache(): Promise<void> {
  const approved = await confirm("将删除处理文件、未收藏的远程原图和缩略图。收藏原图与本地图库文件不会被删除。", { title: "清理壁纸缓存", kind: "warning" });
  if (!approved) return;
  const result = await clearCache();
  cacheInfo.value = await getCacheInfo();
  message.value = `已清理 ${result.removedFiles} 个文件，释放 ${formatBytes(result.freedBytes)}`;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1048576).toFixed(1)} MB`;
  return `${(bytes / 1073741824).toFixed(2)} GB`;
}

/** Stores validated general, sync, scheduler, and cache-limit preferences atomically. */
async function save(): Promise<void> {
  if (!draft.value) return;
  await settingsStore.save(draft.value);
  draft.value = cloneSettings();
  message.value = "设置已保存";
}

/** Selects and immediately indexes one additional LocalProvider root. */
async function addDirectory(): Promise<void> {
  const selected = await open({ directory: true, multiple: false, title: "选择本地壁纸目录" });
  if (!selected) return;
  const count = await scanLocalDirectory(selected);
  await load();
  message.value = `已索引 ${count} 张本地图片`;
}

/** Removes only the tracked root; original images remain untouched. */
async function removeDirectory(path: string): Promise<void> {
  await removeLocalDirectory(path);
  await load();
  message.value = "目录已从配置移除，原始文件未改动";
}

/** Runs the product-required manual metadata refresh using current safe defaults. */
async function refreshResources(): Promise<void> {
  const count = await wallpaperStore.syncOnline({ category: "all", minWidth: 3840, minHeight: 2160, aspectRatio: "16:9", page: 1, pageSize: 24, sort: "latest", safety: "sfw" });
  message.value = `已刷新 ${count} 条 Wallhaven 元数据`;
}

const systemThemeQuery = window.matchMedia("(prefers-color-scheme: light)");
const updateSystemTheme = (event: MediaQueryListEvent): void => { systemPrefersLight.value = event.matches; };

onMounted(() => {
  systemThemeQuery.addEventListener("change", updateSystemTheme);
  void load();
});
onBeforeUnmount(() => {
  systemThemeQuery.removeEventListener("change", updateSystemTheme);
  if (settingsStore.settings) applyTheme(settingsStore.settings);
});
</script>

<template>
  <header class="page-header"><div><p class="eyebrow">SETTINGS</p><h1>设置</h1><p>所有配置仅保存在本机 AppData。</p></div><button :disabled="settingsStore.loading" @click="save">保存设置</button></header>
  <p v-if="settingsStore.error" class="message error">{{ settingsStore.error }}</p><p v-if="message" class="message">{{ message }}</p>
  <div v-if="draft" class="settings-stack">
    <section class="settings-card"><div><h2>常规</h2><p>应用启动和窗口行为。</p></div><div class="toggle-list"><label><input v-model="draft.autoStart" type="checkbox" /><span>开机启动</span></label><label><input v-model="draft.closeToTray" type="checkbox" /><span>关闭窗口时最小化到托盘</span></label></div></section>
    <section class="settings-card theme-settings"><div><h2>应用主题</h2><p>即时预览；保存后在下次启动继续使用。</p></div><div class="theme-editor"><div class="form-grid"><label>主题模式<select v-model="draft.themeMode"><option value="dark">深色</option><option value="light">浅色</option><option value="system">跟随系统</option><option value="custom">自定义配色</option></select></label><label>背景效果<select v-model="draft.themeEffect" :disabled="draft.themeMode === 'system'"><option value="solid">纯色</option><option value="gradient">渐变</option><option value="rainbow">彩虹</option></select></label></div><p v-if="draft.themeMode === 'system'" class="theme-hint">跟随系统使用应用默认配色与纯色背景。</p><div v-if="draft.themeMode === 'custom'" class="color-grid"><label>强调色<input v-model="draft.themeAccent" type="color" /></label><label>辅助色<input v-model="draft.themeSecondary" type="color" /></label><label>背景色<input v-model="draft.themeBackground" type="color" /></label><label>卡片色<input v-model="draft.themeSurface" type="color" /></label></div><div class="theme-preview" :style="themePreviewStyle"><span></span><strong>{{ draft.themeMode === 'system' ? '跟随系统外观' : '主题预览' }}</strong><small>{{ draft.themeEffect === 'solid' ? '纯色' : draft.themeEffect === 'gradient' ? '渐变背景' : '彩虹背景' }}</small></div></div></section>
    <section class="settings-card"><div><h2>自动切换</h2><p>新显示器配置使用的默认值。</p></div><div class="form-grid"><label>默认周期<select v-model.number="draft.wallpaperChangeIntervalSeconds"><option :value="600">10 分钟</option><option :value="1800">30 分钟</option><option :value="3600">1 小时</option><option :value="86400">每天</option><option :value="604800">每周</option></select></label><label>默认适配<select v-model="draft.wallpaperFitMode"><option value="fill">Fill</option><option value="fit">Fit</option><option value="center">Center</option><option value="stretch">Stretch</option></select></label></div></section>
    <section class="settings-card"><div><h2>资源库</h2><p>仅同步 Metadata 和 Thumbnail，不批量下载 4K 原图。</p></div><div class="toggle-list"><label><input v-model="draft.resourceSyncEnabled" type="checkbox" /><span>自动同步</span></label></div><div class="form-grid"><label>同步间隔<select v-model.number="draft.resourceSyncIntervalSeconds"><option :value="21600">6 小时</option><option :value="86400">24 小时</option><option :value="604800">每周</option></select></label><button class="secondary" :disabled="wallpaperStore.syncing" @click="refreshResources">手动刷新</button></div></section>
    <section class="settings-card"><div><h2>缓存</h2><p v-if="cacheInfo">当前 {{ formatBytes(cacheInfo.totalBytes) }} · 原图 {{ formatBytes(cacheInfo.originalBytes) }} · 处理文件 {{ formatBytes(cacheInfo.processedBytes) }} · 缩略图 {{ formatBytes(cacheInfo.thumbnailBytes) }}</p></div><div class="form-grid"><label>缓存上限（{{ cacheLabel }}）<select v-model.number="draft.cacheLimitBytes"><option :value="1073741824">1 GB</option><option :value="5368709120">5 GB</option><option :value="10737418240">10 GB</option><option :value="21474836480">20 GB</option><option :value="0">无限制</option></select></label><button class="secondary" @click="clearApplicationCache">清理缓存</button></div></section>
    <section class="settings-card local-settings"><div><h2>本地图库</h2><p>删除目录只移除跟踪，不删除用户文件。</p></div><button class="secondary" @click="addDirectory">添加目录</button><ul><li v-for="path in draft.localDirectories" :key="path"><span>{{ path }}</span><button class="danger compact" @click="removeDirectory(path)">移除</button></li></ul></section>
  </div>
</template>
