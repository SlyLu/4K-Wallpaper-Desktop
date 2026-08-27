<script setup lang="ts">
import { storeToRefs } from "pinia";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import { getAppStatus, setWallpaper, setWallpaperForMonitor } from "../api/platform";
import type { FitMode } from "../models/image";
import type { AppStatus } from "../models/monitor";
import type { RotationSelectionMode } from "../models/scheduler";
import type { WallpaperRecord } from "../models/wallpaper";
import { useMonitorStore } from "../stores/monitor";
import { useSchedulerStore } from "../stores/scheduler";
import { useWallpaperStore } from "../stores/wallpaper";

const monitorStore = useMonitorStore();
const wallpaperStore = useWallpaperStore();
const schedulerStore = useSchedulerStore();
const { wallpapers, thumbnailUrls, selectedIds, selectedCount, total, loading, error } =
  storeToRefs(wallpaperStore);
const { schedules, pending: schedulerPending, error: schedulerError } =
  storeToRefs(schedulerStore);

const status = ref<AppStatus>();
const imagePath = ref("");
const selectedMonitorId = ref("");
const operationMessage = ref("");
const operationPending = ref(false);
const rotationInterval = ref(30 * 60);
const rotationFitMode = ref<FitMode>("fill");
const rotationSelectionMode = ref<RotationSelectionMode>("round_robin");
const rotationMessage = ref("");
const detailWallpaper = ref<WallpaperRecord>();
const detailOriginalUrl = ref("");
const detailLoading = ref(false);
const detailError = ref("");

const selectedSchedule = computed(() =>
  schedules.value.find((schedule) => schedule.systemMonitorId === selectedMonitorId.value),
);

/** Downloads a card's original on demand, then presents raw IPC bytes as a local Blob URL. */
async function openDetail(wallpaper: WallpaperRecord): Promise<void> {
  closeDetail();
  detailWallpaper.value = wallpaper;
  detailLoading.value = true;
  try {
    const result = await wallpaperStore.downloadOriginal(wallpaper.id);
    detailWallpaper.value = result.wallpaper;
    const blob = new Blob([result.bytes], {
      type: result.wallpaper.mimeType ?? "image/jpeg",
    });
    detailOriginalUrl.value = URL.createObjectURL(blob);
  } catch (cause) {
    detailError.value = String(cause);
  } finally {
    detailLoading.value = false;
  }
}

/** Releases a potentially large 4K Blob immediately when the detail overlay closes. */
function closeDetail(): void {
  if (detailOriginalUrl.value) URL.revokeObjectURL(detailOriginalUrl.value);
  detailOriginalUrl.value = "";
  detailWallpaper.value = undefined;
  detailError.value = "";
  detailLoading.value = false;
}

/** Applies the open catalog item through download, display adaptation, and History. */
async function applyDetailWallpaper(): Promise<void> {
  if (!detailWallpaper.value || !selectedMonitorId.value) return;
  detailLoading.value = true;
  detailError.value = "";
  try {
    await wallpaperStore.apply(
      detailWallpaper.value.id,
      selectedMonitorId.value,
      rotationFitMode.value,
    );
    operationMessage.value = "高清壁纸已应用到选中显示器";
  } catch (cause) {
    detailError.value = String(cause);
  } finally {
    detailLoading.value = false;
  }
}

/** Updates favorite state while keeping the open detail record synchronized. */
async function toggleDetailFavorite(): Promise<void> {
  if (!detailWallpaper.value) return;
  try {
    detailWallpaper.value = await wallpaperStore.toggleFavorite(detailWallpaper.value);
  } catch (cause) {
    detailError.value = String(cause);
  }
}

/** Blacklists the open item and closes it after removal from any selected rotation pool. */
async function blacklistDetail(): Promise<void> {
  if (!detailWallpaper.value) return;
  try {
    await wallpaperStore.blacklist(detailWallpaper.value.id);
    closeDetail();
  } catch (cause) {
    detailError.value = String(cause);
  }
}

/** Persists the explicit selection as one monitor's rotation pool and starts its first run. */
async function configureRotation(): Promise<void> {
  rotationMessage.value = "";
  if (!selectedMonitorId.value) {
    rotationMessage.value = "请先选择显示器";
    return;
  }
  try {
    await schedulerStore.configure(
      selectedMonitorId.value,
      selectedIds.value,
      rotationInterval.value,
      rotationFitMode.value,
      rotationSelectionMode.value,
    );
    rotationMessage.value = selectedIds.value.length
      ? `已为选中显示器启用 ${selectedIds.value.length} 张指定壁纸轮换`
      : "已启用最近 5 张有效壁纸轮换";
  } catch {
    rotationMessage.value = schedulerError.value;
  }
}

/** Runs a scheduler control and surfaces its error beside the rotation configuration. */
async function controlSchedule(action: "pause" | "resume" | "next"): Promise<void> {
  if (!selectedMonitorId.value) return;
  try {
    if (action === "next") await schedulerStore.next(selectedMonitorId.value);
    else await schedulerStore.setPaused(selectedMonitorId.value, action === "pause");
    rotationMessage.value =
      action === "pause" ? "自动切换已暂停" : action === "resume" ? "自动切换已恢复" : "已请求下一张";
  } catch {
    rotationMessage.value = schedulerError.value;
  }
}

/** Converts native invoke failures into a concise validation message. */
async function runWallpaperOperation(operation: () => Promise<void>): Promise<void> {
  operationPending.value = true;
  operationMessage.value = "";
  try {
    await operation();
    operationMessage.value = "壁纸设置成功";
  } catch (cause) {
    operationMessage.value = String(cause);
  } finally {
    operationPending.value = false;
  }
}

/** Applies a user-entered local path to all displays after Rust validation. */
function applyToAll(): void {
  if (!imagePath.value.trim()) {
    operationMessage.value = "请先输入本地图片绝对路径";
    return;
  }
  void runWallpaperOperation(() => setWallpaper(imagePath.value.trim()));
}

/** Applies a user-entered local path only to the selected native display. */
function applyToSelected(): void {
  if (!imagePath.value.trim() || !selectedMonitorId.value) {
    operationMessage.value = "请输入图片路径并选择显示器";
    return;
  }
  void runWallpaperOperation(() =>
    setWallpaperForMonitor(imagePath.value.trim(), selectedMonitorId.value),
  );
}

onMounted(async () => {
  const [appStatus] = await Promise.all([
    getAppStatus(),
    monitorStore.refresh(),
    wallpaperStore.load(),
    schedulerStore.refresh(),
  ]);
  status.value = appStatus;
  selectedMonitorId.value = monitorStore.primaryMonitor?.systemMonitorId ?? "";
});

onBeforeUnmount(closeDetail);
</script>

<template>
  <main class="shell">
    <header class="hero">
      <p class="eyebrow">4K ON DEMAND · SELECTED ROTATION</p>
      <h1>4K Wallpaper Desktop</h1>
      <p class="subtitle">点击预览高清原图，勾选资源建立显示器专属轮换池</p>
    </header>

    <section class="status-card" v-if="status">
      <span class="status-dot" aria-hidden="true"></span>
      <div>
        <strong>{{ status.platform }} Core 已初始化</strong>
        <small>数据目录：{{ status.appDataDirectory }}</small>
      </div>
    </section>

    <section class="section-heading catalog-heading">
      <div>
        <p class="eyebrow">OFFLINE PREVIEW · ORIGINAL ON CLICK</p>
        <h2>精选资源库 <small v-if="total">{{ total }} 张 · 已选 {{ selectedCount }} 张</small></h2>
      </div>
    </section>

    <section class="rotation-panel">
      <div class="rotation-field">
        <label for="rotation-interval">切换周期</label>
        <select id="rotation-interval" v-model.number="rotationInterval">
          <option :value="600">10 分钟</option>
          <option :value="1800">30 分钟</option>
          <option :value="3600">1 小时</option>
          <option :value="21600">6 小时</option>
          <option :value="86400">每天</option>
          <option :value="604800">每周</option>
        </select>
      </div>
      <div class="rotation-field">
        <label for="rotation-selection">选择方式</label>
        <select id="rotation-selection" v-model="rotationSelectionMode">
          <option value="round_robin">轮询</option>
          <option value="random">随机</option>
        </select>
      </div>
      <div class="rotation-field">
        <label for="rotation-fit">适配方式</label>
        <select id="rotation-fit" v-model="rotationFitMode">
          <option value="fill">Fill</option>
          <option value="fit">Fit</option>
          <option value="center">Center</option>
          <option value="stretch">Stretch</option>
        </select>
      </div>
      <button :disabled="schedulerPending" @click="configureRotation">
        {{ selectedCount ? "将已选资源设为动态切换壁纸" : "使用最近 5 张壁纸自动切换" }}
      </button>
      <template v-if="selectedSchedule">
        <button
          class="secondary"
          :disabled="schedulerPending"
          @click="controlSchedule(selectedSchedule.paused ? 'resume' : 'pause')"
        >
          {{ selectedSchedule.paused ? "恢复" : "暂停" }}
        </button>
        <button class="secondary" :disabled="schedulerPending" @click="controlSchedule('next')">
          下一张
        </button>
      </template>
    </section>
    <p v-if="rotationMessage" class="message">{{ rotationMessage }}</p>
    <p v-if="selectedSchedule?.lastError" class="message error">
      上次切换失败：{{ selectedSchedule.lastError }}
    </p>
    <p v-if="error" class="message error">{{ error }}</p>
    <p v-if="loading" class="message">正在加载资源库…</p>

    <div class="preset-grid">
      <article
        v-for="wallpaper in wallpapers"
        :key="wallpaper.id"
        class="preset-card"
        :class="{ selected: selectedIds.includes(wallpaper.id) }"
        tabindex="0"
        @click="openDetail(wallpaper)"
        @keydown.enter="openDetail(wallpaper)"
      >
        <button
          class="selection-toggle"
          :aria-label="selectedIds.includes(wallpaper.id) ? '取消选择' : '选择用于动态切换'"
          @click.stop="wallpaperStore.toggleSelected(wallpaper.id)"
        >
          {{ selectedIds.includes(wallpaper.id) ? "✓" : "+" }}
        </button>
        <span v-if="wallpaper.favorite" class="favorite-badge">♥</span>
        <img :src="thumbnailUrls[wallpaper.id]" :alt="wallpaper.name" loading="lazy" />
        <div>
          <strong>{{ wallpaper.name }}</strong>
          <span>{{ wallpaper.width }} × {{ wallpaper.height }} · {{ wallpaper.category }}</span>
          <small>{{ wallpaper.downloadStatus === "downloaded" ? "高清原图已缓存" : "点击下载高清原图" }}</small>
        </div>
      </article>
    </div>

    <section class="section-heading">
      <div>
        <p class="eyebrow">NATIVE DISPLAYS</p>
        <h2>目标显示器</h2>
      </div>
      <button class="secondary" :disabled="monitorStore.loading" @click="monitorStore.refresh">
        {{ monitorStore.loading ? "检测中…" : "重新检测" }}
      </button>
    </section>

    <p v-if="monitorStore.error" class="message error">{{ monitorStore.error }}</p>
    <div class="monitor-grid">
      <label
        v-for="monitor in monitorStore.monitors"
        :key="monitor.systemMonitorId"
        class="monitor-card"
        :class="{ selected: selectedMonitorId === monitor.systemMonitorId }"
      >
        <input v-model="selectedMonitorId" type="radio" :value="monitor.systemMonitorId" />
        <span class="screen-shape"></span>
        <strong>{{ monitor.name }}</strong>
        <span>{{ monitor.width }} × {{ monitor.height }}</span>
        <small>位置 {{ monitor.positionX }}, {{ monitor.positionY }}</small>
        <em v-if="monitor.primary">主显示器</em>
      </label>
    </div>

    <section class="wallpaper-panel">
      <div>
        <p class="eyebrow">LOCAL IMAGE</p>
        <h2>设置本地壁纸</h2>
        <p>输入本机 jpg、jpeg、png、bmp 或 webp 图片的绝对路径。</p>
      </div>
      <input v-model="imagePath" class="path-input" placeholder="D:\Pictures\wallpaper.jpg" />
      <div class="actions">
        <button :disabled="operationPending" @click="applyToAll">应用到所有显示器</button>
        <button class="secondary" :disabled="operationPending" @click="applyToSelected">
          应用到选中显示器
        </button>
      </div>
      <p v-if="operationMessage" class="message">{{ operationMessage }}</p>
    </section>

    <div v-if="detailWallpaper" class="detail-backdrop" role="presentation" @click="closeDetail">
      <section class="detail-modal" role="dialog" aria-modal="true" @click.stop>
        <button class="detail-close" aria-label="关闭高清预览" @click="closeDetail">×</button>
        <div class="detail-preview">
          <img
            v-if="detailOriginalUrl"
            :src="detailOriginalUrl"
            :alt="detailWallpaper.name"
          />
          <div v-else class="detail-loading">
            {{ detailLoading ? "正在下载并校验高清原图…" : "高清原图暂不可用" }}
          </div>
        </div>
        <div class="detail-info">
          <p class="eyebrow">{{ detailWallpaper.provider }}</p>
          <h2>{{ detailWallpaper.name }}</h2>
          <p>
            {{ detailWallpaper.width }} × {{ detailWallpaper.height }}
            · {{ detailWallpaper.mimeType ?? "image" }}
            · {{ detailWallpaper.category }}
          </p>
          <div v-if="detailWallpaper.tags.length" class="tag-list">
            <span v-for="tag in detailWallpaper.tags" :key="tag">{{ tag }}</span>
          </div>
          <p v-if="detailError" class="message error">{{ detailError }}</p>
          <div class="actions detail-actions">
            <button :disabled="detailLoading || !detailOriginalUrl" @click="applyDetailWallpaper">
              设为选中显示器壁纸
            </button>
            <button class="secondary" :disabled="detailLoading" @click="toggleDetailFavorite">
              {{ detailWallpaper.favorite ? "取消收藏" : "收藏" }}
            </button>
            <button class="danger" :disabled="detailLoading" @click="blacklistDetail">不喜欢</button>
          </div>
        </div>
      </section>
    </div>
  </main>
</template>
