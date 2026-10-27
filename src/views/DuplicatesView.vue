<script setup lang="ts">
// Duplikate (PLAN.md 6.4). Nur Anzeige und Auswahl, keine Dateioperationen.
import { computed, ref, watch } from "vue";
import { api, thumbUrl, type DupeAnalysis, type FileGroup, type FolderInfo } from "../api";
import { useLibraryStore } from "../stores/library";
import { fileName, formatBytes, formatDate, formatInt } from "../format";

const lib = useLibraryStore();
const data = ref<DupeAnalysis | null>(null);
const loading = ref(false);
const error = ref<string | null>(null);
type Tab = "files" | "identical" | "content" | "subset";
const tab = ref<Tab>("files");
const PAGE = 100;
const shown = ref(PAGE);

async function load() {
  loading.value = true;
  try {
    data.value = await api.dupesAnalyze();
    error.value = null;
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}
watch(() => lib.stats, load, { immediate: true });
watch(tab, () => (shown.value = PAGE));

const tabs = computed(() => [
  { id: "files" as const, label: "Dateien", count: data.value?.file_groups.length ?? 0 },
  { id: "identical" as const, label: "Identische Ordner", count: data.value?.identical_folders.length ?? 0 },
  { id: "content" as const, label: "Gleicher Inhalt, andere Namen", count: data.value?.same_content_folders.length ?? 0 },
  { id: "subset" as const, label: "Teilmengen", count: data.value?.subset_folders.length ?? 0 },
]);

function rootName(id: number) {
  const p = lib.rootPath.get(id) ?? "";
  return p.slice(p.lastIndexOf("/") + 1);
}
const fullPath = (rootId: number, rel: string) => `${lib.rootPath.get(rootId) ?? ""}${rel ? "/" + rel : ""}`;
const folderLabel = (f: FolderInfo) => `${rootName(f.key.root_id)}${f.key.rel_path ? " / " + f.key.rel_path.replaceAll("/", " / ") : ""}`;

function keepId(g: FileGroup) {
  return g.keep_asset_id ?? g.files[0].asset_id;
}

async function choose(g: FileGroup, assetId: number) {
  // Vorschlag gewählt = keine eigene Auswahl speichern.
  const value = assetId === g.files[0].asset_id ? null : assetId;
  await api.setDupeChoice(g.hash, value);
  g.keep_asset_id = value;
}
</script>

<template>
  <section class="mx-auto max-w-5xl px-8 py-8">
    <header class="flex flex-wrap items-baseline gap-3">
      <h1 class="text-xl font-semibold tracking-tight">Duplikate</h1>
      <span v-if="data" class="text-stone-500 tabular-nums dark:text-stone-400">{{ formatBytes(data.redundant_size) }} in redundanten Kopien</span>
    </header>
    <p class="mt-2 max-w-2xl text-stone-500 dark:text-stone-400">
      Hier wird nichts gelöscht oder verschoben. Du kannst festlegen, welche Kopie bleiben soll. Die Auswahl wird gespeichert und erst im
      Umsortieren-Planer verwendet, mit Vorschau und Bestätigung.
    </p>

    <p v-if="error" class="mt-4 rounded-md bg-red-50 px-3 py-2 text-red-700 dark:bg-red-500/10 dark:text-red-300">{{ error }}</p>
    <p v-else-if="loading && !data" class="mt-6 text-stone-500">Duplikate werden gesucht …</p>

    <template v-if="data">
      <nav class="mt-5 flex flex-wrap gap-1">
        <button
          v-for="t in tabs"
          :key="t.id"
          class="rounded-md px-2.5 py-1 text-stone-600 hover:bg-stone-200/70 dark:text-stone-400 dark:hover:bg-stone-800"
          :class="tab === t.id && '!bg-stone-200 !text-stone-900 dark:!bg-stone-800 dark:!text-stone-50'"
          @click="tab = t.id"
        >
          {{ t.label }} <span class="text-stone-400 tabular-nums">{{ formatInt(t.count) }}</span>
        </button>
      </nav>

      <!-- Doppelte Dateien -->
      <div v-if="tab === 'files'" class="mt-4 space-y-3">
        <p v-if="!data.file_groups.length" class="py-10 text-center text-stone-500">Keine doppelten Dateien gefunden.</p>
        <article
          v-for="g in data.file_groups.slice(0, shown)"
          :key="g.hash"
          class="flex gap-4 rounded-lg border border-stone-200 bg-white p-3 dark:border-stone-800 dark:bg-stone-900"
        >
          <div class="size-16 shrink-0 overflow-hidden rounded-md bg-stone-200 dark:bg-stone-800">
            <img v-if="g.thumb" :src="thumbUrl(g.thumb)" loading="lazy" class="size-full object-cover" />
          </div>
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-baseline gap-x-3 text-xs text-stone-500">
              <span class="font-medium text-stone-900 dark:text-stone-100">{{ fileName(g.files[0].rel_path) }}</span>
              <span class="tabular-nums">{{ formatBytes(g.size) }} · {{ g.files.length }} Kopien · {{ formatBytes(g.redundant_size) }} einsparbar</span>
            </div>
            <ul class="mt-2 space-y-1">
              <li v-for="(f, i) in g.files" :key="f.asset_id">
                <label class="flex cursor-pointer items-center gap-2 rounded px-1 py-0.5 hover:bg-stone-50 dark:hover:bg-stone-800/60">
                  <input type="radio" :name="g.hash" :checked="keepId(g) === f.asset_id" class="accent-stone-800 dark:accent-stone-200" @change="choose(g, f.asset_id)" />
                  <span class="min-w-0 flex-1 truncate font-mono text-xs select-text" :title="fullPath(f.root_id, f.rel_path)">
                    <span class="text-stone-400">{{ rootName(f.root_id) }}/</span>{{ f.rel_path }}
                  </span>
                  <span v-if="i === 0" class="shrink-0 rounded bg-emerald-50 px-1.5 py-px text-[10px] font-medium text-emerald-800 dark:bg-emerald-500/10 dark:text-emerald-300" :title="g.suggestion_reason">Vorschlag</span>
                  <span v-if="g.keep_asset_id === f.asset_id" class="shrink-0 rounded bg-sky-50 px-1.5 py-px text-[10px] font-medium text-sky-800 dark:bg-sky-500/10 dark:text-sky-300">Deine Wahl</span>
                  <span class="w-28 shrink-0 text-right text-[11px] text-stone-400 tabular-nums">{{ formatDate(f.taken_at) }}</span>
                </label>
              </li>
            </ul>
            <p class="mt-1 text-[11px] text-stone-400">Vorschlag: {{ g.suggestion_reason }}</p>
          </div>
        </article>
        <button
          v-if="data.file_groups.length > shown"
          class="w-full rounded-md border border-stone-300 py-1.5 text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-400 dark:hover:bg-stone-800"
          @click="shown += PAGE"
        >
          Weitere {{ Math.min(PAGE, data.file_groups.length - shown) }} anzeigen ({{ formatInt(data.file_groups.length - shown) }} übrig)
        </button>
      </div>

      <!-- Ordnerpaare -->
      <div v-else-if="tab === 'identical' || tab === 'content'" class="mt-4 space-y-2">
        <p class="text-xs text-stone-500">
          {{ tab === "identical" ? "Gleiche Dateinamen und gleicher Inhalt, inklusive aller Unterordner." : "Gleicher Inhalt, aber Dateien heißen anders." }}
          Unterordner identischer Ordner werden nicht einzeln aufgeführt.
        </p>
        <p v-if="!(tab === 'identical' ? data.identical_folders : data.same_content_folders).length" class="py-10 text-center text-stone-500">Keine gefunden.</p>
        <article
          v-for="(p, i) in (tab === 'identical' ? data.identical_folders : data.same_content_folders).slice(0, shown)"
          :key="i"
          class="rounded-lg border border-stone-200 bg-white px-4 py-3 dark:border-stone-800 dark:bg-stone-900"
        >
          <div class="flex items-center gap-2">
            <span class="min-w-0 flex-1 truncate font-medium select-text" :title="fullPath(p.keep.key.root_id, p.keep.key.rel_path)">{{ folderLabel(p.keep) }}</span>
            <span class="rounded bg-emerald-50 px-1.5 py-px text-[10px] font-medium text-emerald-800 dark:bg-emerald-500/10 dark:text-emerald-300">Vorschlag: behalten</span>
          </div>
          <div class="mt-1 flex items-center gap-2 text-stone-600 dark:text-stone-400">
            <span class="text-stone-400">=</span>
            <span class="min-w-0 flex-1 truncate select-text" :title="fullPath(p.other.key.root_id, p.other.key.rel_path)">{{ folderLabel(p.other) }}</span>
          </div>
          <p class="mt-1.5 text-[11px] text-stone-400 tabular-nums">
            {{ formatInt(p.keep.file_count) }} Dateien · {{ formatBytes(p.keep.size) }} · {{ p.suggestion_reason }}
          </p>
        </article>
      </div>

      <!-- Teilmengen -->
      <div v-else class="mt-4 space-y-2">
        <p class="text-xs text-stone-500">Ordner, deren Inhalt vollständig in einem anderen Ordner steckt, zum Beispiel nach einem abgebrochenen Kopiervorgang.</p>
        <p v-if="!data.subset_folders.length" class="py-10 text-center text-stone-500">Keine gefunden.</p>
        <article
          v-for="(s, i) in data.subset_folders.slice(0, shown)"
          :key="i"
          class="rounded-lg border border-stone-200 bg-white px-4 py-3 dark:border-stone-800 dark:bg-stone-900"
        >
          <div class="truncate font-medium select-text" :title="fullPath(s.subset.key.root_id, s.subset.key.rel_path)">{{ folderLabel(s.subset) }}</div>
          <div class="mt-0.5 truncate text-stone-600 select-text dark:text-stone-400" :title="fullPath(s.superset.key.root_id, s.superset.key.rel_path)">
            steckt vollständig in {{ folderLabel(s.superset) }}
          </div>
          <div class="mt-2 flex items-center gap-3">
            <div class="h-1.5 w-48 overflow-hidden rounded-full" style="background: var(--viz-track)">
              <div class="h-full rounded-full bg-stone-500 dark:bg-stone-400" :style="{ width: `${s.coverage * 100}%` }" />
            </div>
            <span class="text-[11px] text-stone-400 tabular-nums">
              {{ formatInt(s.subset.file_count) }} von {{ formatInt(s.superset.file_count) }} Dateien ({{ Math.round(s.coverage * 100) }} %) · {{ formatBytes(s.subset.size) }}
            </span>
          </div>
        </article>
      </div>
    </template>
  </section>
</template>
