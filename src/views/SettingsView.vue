<script setup lang="ts">
import { confirm, open } from "@tauri-apps/plugin-dialog";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import { removeLocalDirectory, scanLocalDirectory } from "../api/catalog";
import { clearCache, getCacheInfo } from "../api/cache";
import { listProviders, updateProviderConfig } from "../api/providers";
import type { CacheInfo } from "../models/cache";
import type { ProviderStatus } from "../models/provider";
import type { AppConfig } from "../models/settings";
import { useSettingsStore } from "../stores/settings";
import { useWallpaperStore } from "../stores/wallpaper";
import { applyTheme, readableText } from "../utils/theme";
import { BUILTIN_THEMES } from "../themes/builtin";
import { getAppStatus } from "../api/platform";
import type { AppStatus } from "../models/monitor";
import { providerLabel } from "../utils/provider";

const settingsStore = useSettingsStore();
const wallpaperStore = useWallpaperStore();
const draft = ref<AppConfig>();
const message = ref("");
const cacheInfo = ref<CacheInfo>();
const providers = ref<ProviderStatus[]>([]);
const appStatus = ref<AppStatus>();
const systemPrefersLight = ref(window.matchMedia("(prefers-color-scheme: light)").matches);
const cacheLabel = computed(() => draft.value?.cacheLimitBytes === 0 ? "无限制" : `${Math.round(draft.value.cacheLimitBytes / 1073741824)} GB`);
const themeBackgroundName = computed(() => draft.value?.themeBackgroundImage?.split(/[\\/]/).pop() ?? "尚未选择背景图片");

/** Mirrors the production three-layer composition without deriving theme colors from image pixels. */
const themePreviewStyle = computed<Record<string, string>>(() => {
  const settings = draft.value;
  const usesLightDefaults = settings?.themeMode === "light" || (settings?.themeMode === "system" && systemPrefersLight.value);
  const palette = settings?.themeMode === "custom"
    ? { accent: settings.themeAccent, secondary: settings.themeSecondary, background: settings.themeBackground, surface: settings.themeSurface }
    : usesLightDefaults
      ? { accent: "#087f9c", secondary: "#377bd1", background: "#eef6fb", surface: "#ffffff" }
      : { accent: "#64e8f5", secondary: "#4eb2f4", background: "#07111d", surface: "#0a1b29" };
  const sizing = {
    fill: "cover",
    fit: "contain",
    center: "auto",
    stretch: "100% 100%",
  }[settings?.themeBackgroundFit ?? "fill"];
  const previewText = readableText(palette.surface);
  const previewScrim = readableText(palette.background) === "#07111d" ? "#ffffff" : "#000000";
  return {
    "--preview-accent": palette.accent,
    "--preview-secondary": palette.secondary,
    "--preview-bg": palette.background,
    "--preview-surface": palette.surface,
    "--preview-text": previewText,
    "--preview-scrim": previewScrim,
    "--preview-image": settingsStore.backgroundUrl ? `url("${settingsStore.backgroundUrl}")` : "none",
    "--preview-size": sizing,
    "--preview-overlay": String(settings?.themeBackgroundOverlay ?? 0.35),
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
    if (settings) applyTheme(settings, settingsStore.backgroundUrl, settingsStore.backgroundLuminance);
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
  const [, info, providerStatuses, status] = await Promise.all([settingsStore.load(), getCacheInfo(), listProviders(), getAppStatus()]);
  cacheInfo.value = info;
  providers.value = providerStatuses;
  appStatus.value = status;
  draft.value = cloneSettings();
}

/** Updates one independent provider switch and preserves every other source. */
async function toggleProvider(provider: ProviderStatus): Promise<void> {
  if (provider.provider === "thegamesdb" && !provider.enabled) {
    if (!draft.value?.thegamesdbApiKey?.trim()) {
      message.value = "请先填写并保存 TheGamesDB API Key";
      return;
    }
    // The runtime adapter receives the locally persisted key before its first request.
    await save();
  }
  providers.value = await updateProviderConfig(provider.provider, !provider.enabled);
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

/** Imports a bounded AppData copy and previews it without exposing the source path to WebView. */
async function chooseThemeBackground(): Promise<void> {
  if (!draft.value) return;
  const selected = await open({
    multiple: false,
    title: "选择应用背景图片",
    filters: [{ name: "图片", extensions: ["jpg", "jpeg", "png", "webp", "bmp"] }],
  });
  if (!selected) return;
  draft.value.themeBackgroundImage = await settingsStore.importBackground(selected);
}

/** Removes only the preference; normal cache cleanup owns the generated AppData file. */
function clearThemeBackground(): void {
  if (!draft.value) return;
  draft.value.themeBackgroundImage = undefined;
  settingsStore.clearBackgroundPreview();
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
  providers.value = await listProviders();
  message.value = `已聚合刷新 ${count} 条多图源元数据`;
}

const systemThemeQuery = window.matchMedia("(prefers-color-scheme: light)");
const updateSystemTheme = (event: MediaQueryListEvent): void => { systemPrefersLight.value = event.matches; };

onMounted(() => {
  systemThemeQuery.addEventListener("change", updateSystemTheme);
  void load();
});
onBeforeUnmount(() => {
  systemThemeQuery.removeEventListener("change", updateSystemTheme);
  void settingsStore.reloadBackground().then(() => {
    if (settingsStore.settings) applyTheme(settingsStore.settings, settingsStore.backgroundUrl, settingsStore.backgroundLuminance);
  });
});
</script>

<template>
  <header class="page-header"><div><p class="eyebrow">SETTINGS</p><h1>设置</h1><p>所有配置仅保存在本机 AppData。</p></div><button :disabled="settingsStore.loading" @click="save">保存设置</button></header>
  <p v-if="settingsStore.error" class="message error">{{ settingsStore.error }}</p><p v-if="message" class="message">{{ message }}</p>
  <div v-if="draft" class="settings-stack">
    <section class="settings-card"><div><h2>常规</h2><p>应用启动和窗口行为。</p></div><div class="toggle-list"><label><input v-model="draft.autoStart" type="checkbox" /><span>开机启动</span></label><label><input v-model="draft.closeToTray" type="checkbox" /><span>关闭窗口时最小化到托盘</span></label></div></section>
    <section class="settings-card theme-settings">
      <div><h2>应用主题</h2><p>背景图片只保存在本机，预览与应用页面使用相同的遮罩规则。</p></div>
      <div class="theme-editor">
        <div class="form-grid"><label>外观主题<select v-model="draft.themePack"><option v-for="theme in BUILTIN_THEMES" :key="theme.id" :value="theme.id">{{ theme.name }}</option></select></label><label>主题模式<select v-model="draft.themeMode"><option value="dark">深色</option><option value="light">浅色</option><option value="system">跟随系统</option><option value="custom">自定义配色</option></select></label><label>背景效果<select v-model="draft.themeEffect" :disabled="draft.themeMode === 'system'"><option value="solid">纯色</option><option value="gradient">渐变</option><option value="rainbow">彩虹</option></select></label></div>
        <p v-if="draft.themeMode === 'system'" class="theme-hint">跟随系统使用默认配色；背景图片仍可叠加，文字会根据图片明暗自动增强对比度。</p>
        <div v-if="draft.themeMode === 'custom'" class="color-grid"><label>强调色<input v-model="draft.themeAccent" type="color" /></label><label>辅助色<input v-model="draft.themeSecondary" type="color" /></label><label>背景色<input v-model="draft.themeBackground" type="color" /></label><label>卡片色<input v-model="draft.themeSurface" type="color" /></label></div>
        <div class="background-controls">
          <div class="background-file"><button class="secondary" @click="chooseThemeBackground">选择背景图片</button><span :title="draft.themeBackgroundImage">{{ themeBackgroundName }}</span><button v-if="draft.themeBackgroundImage" class="secondary compact" @click="clearThemeBackground">移除</button></div>
          <label>图片适配<select v-model="draft.themeBackgroundFit"><option value="fill">填充屏幕</option><option value="fit">完整显示</option><option value="center">原始尺寸居中</option><option value="stretch">拉伸填满</option></select></label>
          <label class="overlay-control"><span>背景遮罩强度 <strong>{{ Math.round(draft.themeBackgroundOverlay * 100) }}%</strong></span><input v-model.number="draft.themeBackgroundOverlay" type="range" min="0" max="0.85" step="0.05" /><small><em>图片更清晰</em><em>文字更易阅读</em></small></label>
        </div>
        <div class="theme-preview" :style="themePreviewStyle"><div class="theme-preview-surface"><span></span><strong>{{ draft.themeMode === 'system' ? '跟随系统外观' : '主题预览' }}</strong><small>{{ BUILTIN_THEMES.find((theme) => theme.id === draft.themePack)?.name }} · {{ themeBackgroundName }}</small></div></div>
      </div>
    </section>
    <section class="settings-card"><div><h2>自动切换</h2><p>新显示器配置使用的默认值。</p></div><div class="form-grid"><label>默认周期<select v-model.number="draft.wallpaperChangeIntervalSeconds"><option :value="600">10 分钟</option><option :value="1800">30 分钟</option><option :value="3600">1 小时</option><option :value="86400">每天</option><option :value="604800">每周</option></select></label><label>默认适配<select v-model="draft.wallpaperFitMode"><option value="fill">Fill</option><option value="fit">Fit</option><option value="center">Center</option><option value="stretch">Stretch</option></select></label></div></section>
    <section class="settings-card"><div><h2>资源库</h2><p>仅同步 Metadata 和 Thumbnail，不批量下载 4K 原图。</p></div><div class="toggle-list"><label><input v-model="draft.resourceSyncEnabled" type="checkbox" /><span>自动同步</span></label></div><div class="form-grid"><label>同步间隔<select v-model.number="draft.resourceSyncIntervalSeconds"><option :value="21600">6 小时</option><option :value="86400">24 小时</option><option :value="604800">每周</option></select></label><button class="secondary" :disabled="wallpaperStore.syncing" @click="refreshResources">手动刷新</button></div></section>
    <section class="settings-card provider-settings">
      <div><h2>在线图源</h2><p>搜索和刷新会共同查询全部已启用图源，单个失败不影响其他来源。</p></div>
      <div class="provider-editor">
        <label class="provider-key-field"><span>TheGamesDB API Key</span><input v-model="draft.thegamesdbApiKey" type="password" autocomplete="off" placeholder="在 TheGamesDB 登录后获取" /><small>仅保存在本机配置中；用于检索高分辨率游戏 Fanart 和截图。</small></label>
        <div class="provider-list"><button v-for="provider in providers.filter((item) => item.provider !== 'local')" :key="provider.provider" class="provider-row" @click="toggleProvider(provider)"><span><strong>{{ providerLabel(provider.provider) }}</strong><small>{{ provider.status }}<template v-if="provider.responseTimeMs"> · {{ provider.responseTimeMs }}ms</template></small></span><em>{{ provider.enabled ? '已启用' : '已停用' }}</em></button></div>
      </div>
    </section>
    <section class="settings-card"><div><h2>缓存</h2><p v-if="cacheInfo">当前 {{ formatBytes(cacheInfo.totalBytes) }} · 原图 {{ formatBytes(cacheInfo.originalBytes) }} · 处理文件 {{ formatBytes(cacheInfo.processedBytes) }} · 缩略图 {{ formatBytes(cacheInfo.thumbnailBytes) }}</p></div><div class="form-grid"><label>缓存上限（{{ cacheLabel }}）<select v-model.number="draft.cacheLimitBytes"><option :value="1073741824">1 GB</option><option :value="5368709120">5 GB</option><option :value="10737418240">10 GB</option><option :value="21474836480">20 GB</option><option :value="0">无限制</option></select></label><button class="secondary" @click="clearApplicationCache">清理缓存</button></div></section>
    <section class="settings-card local-settings"><div><h2>本地图库</h2><p>删除目录只移除跟踪，不删除用户文件。</p></div><button class="secondary" @click="addDirectory">添加目录</button><ul><li v-for="path in draft.localDirectories" :key="path"><span>{{ path }}</span><button class="danger compact" @click="removeDirectory(path)">移除</button></li></ul></section>
    <section v-if="appStatus" class="settings-card"><div><h2>运行诊断</h2><p>平台 {{ appStatus.platform }} · 数据库 Schema {{ appStatus.schemaVersion }}</p></div><div><p>AppData：{{ appStatus.appDataDirectory }}</p><p>SQLite：{{ appStatus.databasePath }}</p><p>图源状态可在上方独立查看；日志不会上传。</p></div></section>
  </div>
</template>
