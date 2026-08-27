<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";

import type { FitMode } from "../models/image";
import type { RotationSelectionMode } from "../models/scheduler";
import { useMonitorStore } from "../stores/monitor";
import { useSchedulerStore } from "../stores/scheduler";
import { useWallpaperStore } from "../stores/wallpaper";

const monitorStore = useMonitorStore();
const schedulerStore = useSchedulerStore();
const wallpaperStore = useWallpaperStore();
const mode = ref<"unified" | "independent">("independent");
const selectedMonitorId = ref("");
const intervalSeconds = ref(1800);
const fitMode = ref<FitMode>("fill");
const selectionMode = ref<RotationSelectionMode>("round_robin");
const message = ref("");
const selectedSchedule = computed(() => schedulerStore.schedules.find((item) => item.systemMonitorId === selectedMonitorId.value));

/** Returns the persisted schedule belonging to one physical display. */
function scheduleForMonitor(monitorId: string) {
  return schedulerStore.schedules.find((item) => item.systemMonitorId === monitorId);
}

/** Restores independent form values whenever the selected display or persisted data changes. */
watch(
  [selectedSchedule, mode],
  ([schedule, currentMode]) => {
    if (currentMode !== "independent") return;
    intervalSeconds.value = schedule?.intervalSeconds ?? 1800;
    fitMode.value = schedule?.fitMode ?? "fill";
    selectionMode.value = schedule?.selectionMode ?? "round_robin";
  },
  { immediate: true },
);

/** Persists either one per-display schedule or the same pool for every active display. */
async function configure(): Promise<void> {
  const targets = mode.value === "unified" ? monitorStore.monitors.map((item) => item.systemMonitorId) : [selectedMonitorId.value];
  if (targets.some((item) => !item)) return;
  try {
    for (const monitorId of targets) {
      await schedulerStore.configure(
        monitorId,
        wallpaperStore.selectedIds,
        intervalSeconds.value,
        fitMode.value,
        selectionMode.value,
      );
    }
    const source = wallpaperStore.selectedIds.length
      ? `${wallpaperStore.selectedIds.length} 张指定壁纸`
      : "最近使用的 5 张有效壁纸";
    message.value = mode.value === "unified" ? `已为 ${targets.length} 块显示器配置：${source}` : `轮换已保存：${source}`;
  } catch {
    message.value = schedulerStore.error;
  }
}

/** Controls only the currently selected display schedule. */
async function control(action: "pause" | "resume" | "next"): Promise<void> {
  if (!selectedMonitorId.value) return;
  if (action === "next") await schedulerStore.next(selectedMonitorId.value);
  else await schedulerStore.setPaused(selectedMonitorId.value, action === "pause");
  message.value = action === "next" ? "已请求下一张" : action === "pause" ? "轮换已暂停" : "轮换已恢复";
}

onMounted(async () => {
  await Promise.all([monitorStore.refresh(), schedulerStore.refresh()]);
  selectedMonitorId.value = monitorStore.primaryMonitor?.systemMonitorId ?? monitorStore.monitors[0]?.systemMonitorId ?? "";
});
</script>

<template>
  <header class="page-header"><div><p class="eyebrow">NATIVE DISPLAYS</p><h1>显示器与自动切换</h1><p>每块屏幕拥有独立周期、来源池和适配方式。</p></div><button class="secondary" @click="monitorStore.refresh">重新检测</button></header>
  <div class="mode-switch"><button :class="{ active: mode === 'unified' }" @click="mode = 'unified'">统一模式</button><button :class="{ active: mode === 'independent' }" @click="mode = 'independent'">独立模式</button></div>
  <div class="display-grid">
    <button v-for="monitor in monitorStore.monitors" :key="monitor.systemMonitorId" class="display-card" :class="{ selected: selectedMonitorId === monitor.systemMonitorId }" @click="selectedMonitorId = monitor.systemMonitorId">
      <span class="monitor-visual" :style="{ aspectRatio: `${monitor.width}/${monitor.height}` }"></span>
      <strong>{{ monitor.name }}</strong><span>{{ monitor.width }} × {{ monitor.height }}</span><small>{{ monitor.primary ? "主显示器" : `位置 ${monitor.positionX}, ${monitor.positionY}` }}</small>
      <em v-if="scheduleForMonitor(monitor.systemMonitorId)">已配置 · {{ scheduleForMonitor(monitor.systemMonitorId)?.selectionMode === 'random' ? '随机' : '轮询' }}</em>
    </button>
  </div>
  <section class="settings-card rotation-config">
    <div><p class="eyebrow">ROTATION POOL</p><h2>{{ mode === 'unified' ? '所有显示器统一配置' : '选中显示器独立配置' }}</h2><p v-if="wallpaperStore.selectedCount">使用当前勾选的 {{ wallpaperStore.selectedCount }} 张在线/本地资源。</p><p v-else>未勾选资源，将使用最近设置过的 5 张有效壁纸。</p></div>
    <div class="form-grid"><label>切换周期<select v-model.number="intervalSeconds"><option :value="600">10 分钟</option><option :value="1800">30 分钟</option><option :value="3600">1 小时</option><option :value="86400">每天</option><option :value="604800">每周</option></select></label><label>选择方式<select v-model="selectionMode"><option value="round_robin">轮询</option><option value="random">随机</option></select></label><label>Fit Mode<select v-model="fitMode"><option value="fill">Fill</option><option value="fit">Fit</option><option value="center">Center</option><option value="stretch">Stretch</option></select></label></div>
    <div class="actions"><button :disabled="schedulerStore.pending" @click="configure">保存并启用</button><template v-if="selectedSchedule"><button class="secondary" @click="control(selectedSchedule.paused ? 'resume' : 'pause')">{{ selectedSchedule.paused ? '恢复' : '暂停' }}</button><button class="secondary" @click="control('next')">下一张</button></template></div>
    <p v-if="message" class="message">{{ message }}</p><p v-if="selectedSchedule?.lastError" class="message error">上次错误：{{ selectedSchedule.lastError }}</p>
  </section>
</template>
