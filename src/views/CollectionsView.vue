<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";

import {
  createCollection,
  deleteCollection,
  listCollections,
  setSmartCollectionRule,
} from "../api/collections";
import WallpaperGrid from "../components/WallpaperGrid.vue";
import type { CollectionRecord, SmartCollectionRule } from "../models/collection";
import { useWallpaperStore } from "../stores/wallpaper";

const wallpaperStore = useWallpaperStore();
const collections = ref<CollectionRecord[]>([]);
const active = ref<CollectionRecord>();
const browseAll = ref(false);
const creating = ref(false);
const message = ref("");
const form = reactive({ name: "", description: "", smart: false, category: "all", provider: "all", favorite: false, tags: "" });
const bulkMode = computed(() => browseAll.value ? "collection-add" as const : "collection-remove" as const);

/** Reloads summaries so counts immediately reflect membership mutations. */
async function refreshCollections(): Promise<void> {
  collections.value = await listCollections();
  if (active.value) active.value = collections.value.find((item) => item.id === active.value?.id);
}

/** Opens one collection using its manual membership or persisted smart rule. */
async function selectCollection(collection: CollectionRecord): Promise<void> {
  active.value = collection;
  browseAll.value = false;
  wallpaperStore.clearBulkSelected();
  await wallpaperStore.queryCollection(collection.id);
}

/** Creates manual or schema-versioned smart collections from one compact form. */
async function submitCollection(): Promise<void> {
  creating.value = true;
  message.value = "";
  try {
    const collection = await createCollection(form.name, form.description);
    if (form.smart) {
      const rule: SmartCollectionRule = {
        version: 1,
        category: form.category === "all" ? undefined : form.category,
        provider: form.provider === "all" ? undefined : form.provider,
        favorite: form.favorite || undefined,
        tags: form.tags.split(",").map((tag) => tag.trim()).filter(Boolean),
      };
      await setSmartCollectionRule(collection.id, rule);
    }
    form.name = "";
    form.description = "";
    await refreshCollections();
    const created = collections.value.find((item) => item.id === collection.id);
    if (created) await selectCollection(created);
    message.value = "集合已创建";
  } catch (cause) {
    message.value = String(cause);
  } finally {
    creating.value = false;
  }
}

/** Deletes the container only after explaining that wallpapers remain untouched. */
async function removeActive(): Promise<void> {
  if (!active.value || !window.confirm(`删除集合“${active.value.name}”？壁纸和文件不会被删除。`)) return;
  await deleteCollection(active.value.id);
  active.value = undefined;
  wallpaperStore.clearBulkSelected();
  await Promise.all([refreshCollections(), wallpaperStore.query({ pageSize: 60 })]);
}

/** Switches to the complete catalog so a cross-page selection can be added. */
async function browseCatalog(): Promise<void> {
  browseAll.value = true;
  wallpaperStore.clearBulkSelected();
  await wallpaperStore.query({ pageSize: 60 });
}

onMounted(async () => {
  await refreshCollections();
  if (collections.value[0]) await selectCollection(collections.value[0]);
});
</script>

<template>
  <header class="page-header"><div><p class="eyebrow">COLLECTIONS</p><h1>壁纸集合</h1><p>用手动集合或安全的智能规则组织每块屏幕的轮换来源。</p></div></header>
  <section class="collection-create">
    <input v-model="form.name" maxlength="80" placeholder="集合名称" />
    <input v-model="form.description" placeholder="说明（可选）" />
    <label><input v-model="form.smart" type="checkbox" /> 智能集合</label>
    <template v-if="form.smart"><select v-model="form.category"><option value="all">全部分类</option><option value="nature">自然</option><option value="anime">动漫</option><option value="people">人物</option><option value="local">本地</option></select><select v-model="form.provider"><option value="all">全部来源</option><option value="wallhaven">Wallhaven</option><option value="wikimedia_commons">Wikimedia Commons</option><option value="local">Local</option></select><input v-model="form.tags" placeholder="标签，逗号分隔" /><label><input v-model="form.favorite" type="checkbox" /> 仅收藏</label></template>
    <button :disabled="creating || !form.name.trim()" @click="submitCollection">{{ creating ? "创建中…" : "创建集合" }}</button>
  </section>
  <p v-if="message" class="inline-status">{{ message }}</p>
  <section class="collection-layout">
    <aside class="collection-list"><button v-for="collection in collections" :key="collection.id" :class="{ active: active?.id === collection.id }" @click="selectCollection(collection)"><strong>{{ collection.name }}</strong><span>{{ collection.smart ? "智能" : "手动" }} · {{ collection.wallpaperCount }} 张</span></button><p v-if="!collections.length">还没有集合</p></aside>
    <div class="collection-content">
      <div v-if="active" class="section-title"><div><h2>{{ active.name }}</h2><p>{{ active.description || (active.smart ? "按规则实时生成" : "手动管理") }}</p></div><div class="actions"><button v-if="!active.smart" class="secondary" @click="browseCatalog">从图库添加</button><button v-if="browseAll" class="secondary" @click="selectCollection(active)">返回集合</button><button class="danger" @click="removeActive">删除集合</button></div></div>
      <WallpaperGrid v-if="active" :bulk-mode="active.smart ? undefined : bulkMode" :collection-id="active.id" :empty-text="browseAll ? '图库中没有可添加的壁纸。' : '这个集合还是空的。'" />
      <div v-else class="empty-state"><strong>创建第一个集合</strong><p>集合只保存组织关系，不复制或删除原图。</p></div>
    </div>
  </section>
</template>
