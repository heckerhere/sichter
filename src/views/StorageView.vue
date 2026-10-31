<script setup lang="ts">
// Speicherübersicht (PLAN.md 6.6). Reine Leseansicht.
import { computed, ref, watch } from "vue";
import { api, type FolderRow, type LargeFile, type StorageSummary } from "../api";
import { useLibraryStore } from "../stores/library";
import { byCategory, categoryOf, cssVar } from "../categories";
import { fileName, formatBytes, formatInt } from "../format";
import { t } from "../i18n";
import FolderTree from "../components/FolderTree.vue";
import KindBar from "../components/KindBar.vue";
import StorageChart from "../components/StorageChart.vue";

const lib = useLibraryStore();
const rootId = ref<number | null>(null);
const folders = ref<FolderRow[]>([]);
const summary = ref<StorageSummary | null>(null);
const largest = ref<LargeFile[]>([]);
const error = ref<string | null>(null);
const view = ref<"tree" | "treemap" | "sunburst">("tree");

const root = computed(() => lib.roots.find((r) => r.id === rootId.value) ?? null);
const rootLabel = computed(() => (root.value ? root.value.path.slice(root.value.path.lastIndexOf("/") + 1) : ""));
const rootRow = computed(() => folders.value.find((f) => f.rel_path === ""));
const breakdown = computed(() => {
  const cats = byCategory(rootRow.value?.kinds ?? []);
  const total = cats.reduce((s, c) => s + c.size, 0);
  return cats.map((c) => ({ ...c, share: total ? c.size / total : 0 }));
});
const topFolders = computed(() =>
  folders.value
    .filter((f) => f.rel_path !== "")
    .sort((a, b) => b.total_size - a.total_size)
    .slice(0, 10),
);
const topFiles = computed(() => largest.value.filter((f) => f.root_id === rootId.value).slice(0, 10));

async function load() {
  if (rootId.value === null) return;
  try {
    [folders.value, summary.value, largest.value] = await Promise.all([
      api.storageFolders(rootId.value),
      api.storageSummary(),
      api.largestFiles(200),
    ]);
    error.value = null;
  } catch (e) {
    error.value = String(e);
  }
}

// Ersten Root wählen, sobald Roots bekannt sind; nach jedem Scan neu laden.
watch(
  () => lib.roots,
  (roots) => {
    if (!roots.some((r) => r.id === rootId.value)) rootId.value = roots[0]?.id ?? null;
  },
  { immediate: true },
);
watch([rootId, () => lib.stats], load, { immediate: true });

const views = ["tree", "treemap", "sunburst"] as const;
</script>

<template>
  <section class="mx-auto max-w-6xl px-8 py-8">
    <header class="flex flex-wrap items-center gap-3">
      <h1 class="text-xl font-semibold tracking-tight">{{ t.storage.title }}</h1>
      <div v-if="lib.roots.length > 1" class="ml-4 flex flex-wrap gap-1">
        <button
          v-for="r in lib.roots"
          :key="r.id"
          class="max-w-64 truncate rounded-md px-2.5 py-1 text-stone-600 hover:bg-stone-200/70 dark:text-stone-400 dark:hover:bg-stone-800"
          :class="rootId === r.id && '!bg-stone-200 !text-stone-900 dark:!bg-stone-800 dark:!text-stone-50'"
          :title="r.path"
          @click="rootId = r.id"
        >
          {{ r.path.slice(r.path.lastIndexOf("/") + 1) }}
        </button>
      </div>
      <span class="ml-auto text-xs text-stone-400">{{ t.storage.readOnly }}</span>
    </header>

    <p v-if="error" class="mt-4 rounded-md bg-red-50 px-3 py-2 text-red-700 dark:bg-red-500/10 dark:text-red-300">{{ error }}</p>

    <div v-if="!lib.roots.length" class="mt-6 rounded-lg border border-dashed border-stone-300 px-6 py-10 text-center text-stone-500 dark:border-stone-700">
      {{ t.storage.emptyBefore }}<RouterLink to="/" class="underline underline-offset-2">{{ t.storage.emptyLink }}</RouterLink>{{ t.storage.emptyAfter }}
    </div>
    <div v-else-if="!rootRow" class="mt-6 rounded-lg border border-dashed border-stone-300 px-6 py-10 text-center text-stone-500 dark:border-stone-700">
      {{ t.storage.noScanData }}
    </div>

    <template v-else>
      <!-- Überblick: Gesamtgröße, Aufteilung nach Typ, Einsparbar -->
      <div class="mt-5 grid gap-4 lg:grid-cols-[1fr_320px]">
        <div class="rounded-lg border border-stone-200 bg-white p-5 dark:border-stone-800 dark:bg-stone-900">
          <div class="flex items-baseline gap-3">
            <span class="text-3xl font-semibold tracking-tight tabular-nums">{{ formatBytes(rootRow.total_size) }}</span>
            <span class="text-stone-500 tabular-nums">{{ t.storage.files(formatInt(rootRow.file_count)) }}</span>
          </div>
          <div class="mt-4"><KindBar :kinds="rootRow.kinds" :height="14" /></div>
          <ul class="mt-4 grid grid-cols-2 gap-x-6 gap-y-2 sm:grid-cols-3 xl:grid-cols-5">
            <li v-for="c in breakdown" :key="c.id" :title="c.hint" :class="!c.size && 'opacity-45'">
              <div class="flex items-center gap-1.5 text-xs text-stone-600 dark:text-stone-400">
                <span class="size-2.5 rounded-sm" :style="{ background: cssVar(c.id) }" />{{ c.label }}
              </div>
              <div class="mt-0.5 font-medium tabular-nums">{{ formatBytes(c.size) }}</div>
              <div class="text-[11px] text-stone-400 tabular-nums">{{ t.storage.shareFiles(Math.round(c.share * 100), formatInt(c.count)) }}</div>
            </li>
          </ul>
        </div>

        <div class="rounded-lg border border-stone-200 bg-white p-5 dark:border-stone-800 dark:bg-stone-900">
          <h2 class="text-xs font-medium tracking-wide text-stone-500 uppercase dark:text-stone-400">{{ t.storage.reclaimable }}</h2>
          <dl class="mt-3 space-y-3">
            <div>
              <dt class="text-stone-600 dark:text-stone-400">{{ t.storage.duplicateFiles }}</dt>
              <dd class="font-medium tabular-nums">{{ formatBytes(summary?.duplicate_size ?? 0) }}</dd>
              <dd class="text-[11px] text-stone-400">
                {{ t.storage.duplicateHint }}
                <RouterLink to="/duplikate" class="underline underline-offset-2">{{ t.storage.view }}</RouterLink>
              </dd>
            </div>
            <div>
              <dt class="text-stone-600 dark:text-stone-400">{{ t.storage.rejected }}</dt>
              <dd class="font-medium tabular-nums">{{ formatBytes(summary?.reject_size ?? 0) }}</dd>
              <dd class="text-[11px] text-stone-400">
                {{ t.storage.rejectHint(formatInt(summary?.reject_count ?? 0)) }}
                <RouterLink to="/sichten" class="underline underline-offset-2">{{ t.storage.cull }}</RouterLink>
              </dd>
            </div>
            <div v-if="summary && summary.raw_pair_count" class="border-t border-stone-100 pt-3 dark:border-stone-800">
              <dt class="text-stone-600 dark:text-stone-400">{{ t.storage.rawPairs }}</dt>
              <dd class="font-medium tabular-nums">{{ formatBytes(summary.raw_pair_jpeg_size) }}</dd>
              <dd class="text-[11px] text-stone-400">
                {{ t.storage.rawPairsHint(formatInt(summary.raw_pair_count)) }}
              </dd>
            </div>
          </dl>
        </div>
      </div>

      <!-- Ordner -->
      <div class="mt-6 flex items-center gap-1">
        <button
          v-for="v in views"
          :key="v"
          class="rounded-md px-2.5 py-1 text-stone-600 hover:bg-stone-200/70 dark:text-stone-400 dark:hover:bg-stone-800"
          :class="view === v && '!bg-stone-200 !text-stone-900 dark:!bg-stone-800 dark:!text-stone-50'"
          @click="view = v"
        >
          {{ t.storage.views[v] }}
        </button>
        <span v-if="view !== 'tree'" class="ml-3 text-xs text-stone-400">{{ t.storage.zoomHint }}</span>
      </div>
      <div class="mt-3">
        <FolderTree v-if="view === 'tree'" :folders="folders" :root-label="rootLabel" />
        <div v-else class="h-[480px] rounded-lg border border-stone-200 bg-white p-3 dark:border-stone-800 dark:bg-stone-900">
          <StorageChart :folders="folders" :root-label="rootLabel" :mode="view" />
        </div>
      </div>

      <!-- Top-Listen -->
      <div class="mt-6 grid gap-4 lg:grid-cols-2">
        <div class="rounded-lg border border-stone-200 bg-white dark:border-stone-800 dark:bg-stone-900">
          <h2 class="border-b border-stone-100 px-4 py-2.5 text-xs font-medium tracking-wide text-stone-500 uppercase dark:border-stone-800 dark:text-stone-400">{{ t.storage.largestFolders }}</h2>
          <ol>
            <li v-for="f in topFolders" :key="f.id" class="flex items-center gap-3 border-b border-stone-50 px-4 py-1.5 last:border-0 dark:border-stone-800/60">
              <span class="min-w-0 flex-1 truncate" :title="f.rel_path">{{ f.rel_path }}</span>
              <div class="w-24"><KindBar :kinds="f.kinds" :height="6" /></div>
              <span class="w-20 text-right tabular-nums">{{ formatBytes(f.total_size) }}</span>
            </li>
            <li v-if="!topFolders.length" class="px-4 py-3 text-stone-400">{{ t.storage.noSubfolders }}</li>
          </ol>
        </div>
        <div class="rounded-lg border border-stone-200 bg-white dark:border-stone-800 dark:bg-stone-900">
          <h2 class="border-b border-stone-100 px-4 py-2.5 text-xs font-medium tracking-wide text-stone-500 uppercase dark:border-stone-800 dark:text-stone-400">{{ t.storage.largestFiles }}</h2>
          <ol>
            <li v-for="f in topFiles" :key="f.rel_path" class="flex items-center gap-3 border-b border-stone-50 px-4 py-1.5 last:border-0 dark:border-stone-800/60">
              <span class="size-2 shrink-0 rounded-sm" :style="{ background: cssVar(categoryOf(f.kind)) }" :title="f.kind" />
              <span class="min-w-0 flex-1 truncate" :title="f.rel_path">{{ fileName(f.rel_path) }}</span>
              <span class="max-w-48 truncate text-xs text-stone-400" :title="f.rel_path">{{ f.rel_path.includes("/") ? f.rel_path.slice(0, f.rel_path.lastIndexOf("/")) : "" }}</span>
              <span class="w-20 text-right tabular-nums">{{ formatBytes(f.size) }}</span>
            </li>
          </ol>
        </div>
      </div>
    </template>
  </section>
</template>
