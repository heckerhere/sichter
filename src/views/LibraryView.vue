<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { ask, open } from "@tauri-apps/plugin-dialog";
import { useLibraryStore } from "../stores/library";
import type { GridItem } from "../api";
import { formatBytes, formatInt } from "../format";
import VirtualGrid from "../components/VirtualGrid.vue";
import GridTile from "../components/GridTile.vue";
import SystemStatus from "../components/SystemStatus.vue";
import PhotosImportDialog from "../components/PhotosImportDialog.vue";
import { api } from "../api";
import { PHASES, overallPercent, phaseLabel, phaseStep } from "../scanPhases";
import { t } from "../i18n";

const lib = useLibraryStore();
const router = useRouter();

// Import aus der Fotos-Mediathek gibt es nur unter macOS.
const photosSupported = ref(false);
const showPhotosImport = ref(false);
onMounted(async () => (photosSupported.value = (await api.photosStatus().catch(() => null))?.supported ?? false));

async function onImportPlanned(runId: number) {
  showPhotosImport.value = false;
  await router.push(`/plan/${runId}`);
}

const addErrors = ref<string[]>([]);
const showUnhandled = ref(false);
const showErrors = ref(false);

type Filter = "all" | "photo" | "raw" | "video" | "uncertain";
const filter = ref<Filter>("all");
const filters: { id: Filter; test: (i: GridItem) => boolean }[] = [
  { id: "all", test: () => true },
  { id: "photo", test: (i) => i.kind !== "video" && i.kind !== "raw" },
  { id: "raw", test: (i) => i.kinds.includes("raw") },
  { id: "video", test: (i) => i.kind === "video" },
  { id: "uncertain", test: (i) => !!i.date_source && i.date_source !== "exif" },
];
const counts = computed(() => Object.fromEntries(filters.map((f) => [f.id, lib.groups.filter(f.test).length])));
const visibleItems = computed(() => lib.groups.filter(filters.find((f) => f.id === filter.value)!.test));

const percent = computed(() => overallPercent(lib.progress));
const step = computed(() => phaseStep(lib.progress?.phase));
const steps = PHASES.filter((p) => p.id !== "done");
const currentIndex = computed(() => steps.findIndex((p) => p.id === lib.progress?.phase));

async function addFolders() {
  const picked = await open({ directory: true, multiple: true, title: t.library.addFoldersTitle });
  if (!picked) return;
  addErrors.value = await lib.addRoots(Array.isArray(picked) ? picked : [picked]);
}

async function removeRoot(id: number, path: string) {
  const ok = await ask(
    t.library.removeRoot.message(path),
    { title: t.library.removeRoot.title, kind: "warning", okLabel: t.library.removeRoot.ok, cancelLabel: t.common.cancel },
  );
  if (ok) await lib.removeRoot(id);
}
</script>

<template>
  <section class="flex h-full flex-col">
    <header class="flex items-center gap-4 px-8 pt-8 pb-4">
      <div>
        <h1 class="text-xl font-semibold tracking-tight">{{ t.library.title }}</h1>
        <p v-if="lib.stats && lib.stats.files" class="mt-0.5 text-stone-500 tabular-nums dark:text-stone-400">
          {{ t.library.stats(formatInt(lib.stats.groups), formatInt(lib.stats.files), formatBytes(lib.stats.total_size)) }}
        </p>
      </div>
      <div class="ml-auto flex gap-2">
        <button
          v-if="lib.roots.length"
          :disabled="lib.scanning"
          class="rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 disabled:opacity-40 dark:border-stone-700 dark:hover:bg-stone-800"
          @click="lib.startScan()"
        >
          {{ t.library.rescan }}
        </button>
        <button
          v-if="photosSupported"
          :disabled="lib.scanning"
          class="rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 disabled:opacity-40 dark:border-stone-700 dark:hover:bg-stone-800"
          @click="showPhotosImport = true"
        >
          {{ t.library.importPhotos }}
        </button>
        <button
          :disabled="lib.scanning"
          class="rounded-md bg-stone-900 px-3 py-1.5 text-white hover:bg-stone-700 disabled:opacity-40 dark:bg-stone-100 dark:text-stone-900 dark:hover:bg-white"
          @click="addFolders"
        >
          {{ t.library.addFolders }}
        </button>
      </div>
    </header>

    <div class="space-y-3 px-8">
      <p v-for="e in addErrors" :key="e" class="rounded-md bg-red-50 px-3 py-2 text-red-700 dark:bg-red-500/10 dark:text-red-300">{{ e }}</p>
      <p v-if="lib.error" class="rounded-md bg-red-50 px-3 py-2 text-red-700 dark:bg-red-500/10 dark:text-red-300">{{ lib.error }}</p>

      <!-- Quellordner -->
      <ul v-if="lib.roots.length" class="divide-y divide-stone-100 rounded-lg border border-stone-200 bg-white dark:divide-stone-800 dark:border-stone-800 dark:bg-stone-900">
        <li v-for="r in lib.roots" :key="r.id" class="group flex items-center gap-3 px-4 py-2">
          <span class="min-w-0 flex-1 truncate font-mono text-xs select-text" :title="r.path">{{ r.path }}</span>
          <span v-if="!r.reachable" class="rounded bg-amber-100 px-1.5 py-px text-[11px] text-amber-800 dark:bg-amber-500/15 dark:text-amber-300">{{ t.library.root.unreachable }}</span>
          <button
            class="rounded px-1.5 py-px text-[11px]"
            :class="r.locked ? 'bg-stone-800 text-white dark:bg-stone-200 dark:text-stone-900' : 'text-stone-400 opacity-0 group-hover:opacity-100 hover:text-stone-700 dark:hover:text-stone-200'"
            :title="r.locked ? t.library.root.lockedTitle : t.library.root.lockTitle"
            @click="lib.setLocked(r.id, !r.locked)"
          >
            {{ r.locked ? t.library.root.locked : t.library.root.lock }}
          </button>
          <span class="text-xs text-stone-500 tabular-nums">{{ t.library.root.files(formatInt(r.file_count), formatBytes(r.total_size)) }}</span>
          <button
            :disabled="lib.scanning"
            class="text-xs text-stone-400 opacity-0 group-hover:opacity-100 hover:text-red-600 disabled:hidden"
            @click="removeRoot(r.id, r.path)"
          >
            {{ t.common.remove }}
          </button>
        </li>
      </ul>

      <!-- Scan-Fortschritt -->
      <div v-if="lib.scanning" class="rounded-lg border border-stone-200 bg-white px-4 py-3 dark:border-stone-800 dark:bg-stone-900">
        <div class="flex items-baseline gap-3">
          <span class="text-lg font-semibold tabular-nums">{{ t.library.scan.percent(percent) }}</span>
          <span class="font-medium">{{ t.library.scan.step(step.step, step.of, phaseLabel(lib.progress?.phase)) }}</span>
          <span v-if="lib.progress && lib.progress.done" class="text-stone-500 tabular-nums">
            {{ formatInt(lib.progress.done) }}<template v-if="lib.progress.total"> / {{ formatInt(lib.progress.total) }}</template>
          </span>
          <button class="ml-auto self-center rounded-md border border-stone-300 px-2.5 py-1 text-xs hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="lib.cancelScan">
            {{ t.common.cancel }}
          </button>
        </div>
        <div class="mt-2 h-2 overflow-hidden rounded-full bg-stone-100 dark:bg-stone-800">
          <div class="h-full rounded-full bg-sky-500 transition-[width] duration-300" :style="{ width: `${Math.max(1, percent)}%` }" />
        </div>
        <ol class="mt-2.5 flex flex-wrap gap-x-4 gap-y-1 text-xs">
          <li
            v-for="(s, i) in steps"
            :key="s.id"
            :class="i < currentIndex ? 'text-stone-500' : i === currentIndex ? 'font-medium text-sky-700 dark:text-sky-400' : 'text-stone-300 dark:text-stone-600'"
          >
            <span v-if="i < currentIndex">✓ </span>{{ s.label }}
          </li>
        </ol>
        <p class="mt-1.5 h-4 truncate font-mono text-[11px] text-stone-400">{{ lib.progress?.current }}</p>
      </div>

      <!-- Ergebnis des letzten Scans -->
      <div v-else-if="lib.lastReport" class="rounded-lg border border-stone-200 bg-white px-4 py-3 text-xs text-stone-600 dark:border-stone-800 dark:bg-stone-900 dark:text-stone-400">
        <span class="font-medium text-stone-900 dark:text-stone-100">{{ lib.lastReport.cancelled ? t.library.report.cancelled : t.library.report.finished }}</span>
        · {{ t.library.report.new(formatInt(lib.lastReport.new)) }} · {{ t.library.report.changed(formatInt(lib.lastReport.changed)) }} · {{ t.library.report.removed(formatInt(lib.lastReport.removed)) }}
        · {{ t.library.report.seconds((lib.lastReport.duration_ms / 1000).toFixed(1)) }}
        <span v-if="lib.lastReport.cancelled"> · {{ t.library.report.resume }}</span>
        <div v-for="r in lib.lastReport.unreachable_roots" :key="r" class="mt-1 text-amber-700 dark:text-amber-400">
          {{ t.library.report.unreachable(r) }}
        </div>
        <div v-for="p in lib.lastReport.skipped_packages" :key="p" class="mt-1">{{ t.library.report.skippedPackage(p) }}</div>
        <div v-if="lib.lastReport.errors.length" class="mt-1">
          <button class="text-red-600 underline-offset-2 hover:underline" @click="showErrors = !showErrors">
            {{ t.library.report.errors(lib.lastReport.errors.length) }}
          </button>
          <ul v-if="showErrors" class="mt-1 max-h-40 overflow-y-auto font-mono select-text">
            <li v-for="([path, msg], i) in lib.lastReport.errors" :key="i">{{ path }}: {{ msg }}</li>
          </ul>
        </div>
      </div>
    </div>

    <!-- Leerer Zustand -->
    <div v-if="!lib.roots.length" class="mx-auto mt-6 w-full max-w-3xl px-8 pb-10">
      <div class="rounded-lg border border-dashed border-stone-300 px-6 py-10 text-center dark:border-stone-700">
        <p class="font-medium">{{ t.library.empty.title }}</p>
        <p class="mx-auto mt-1 max-w-sm text-stone-500 dark:text-stone-400">
          {{ t.library.empty.text }}
        </p>
        <p v-if="photosSupported" class="mx-auto mt-3 max-w-sm text-stone-500 dark:text-stone-400">
          {{ t.library.empty.photosQuestion }}
          <button class="text-sky-700 underline-offset-2 hover:underline dark:text-sky-400" @click="showPhotosImport = true">{{ t.library.empty.photosLink }}</button>
        </p>
      </div>
      <h2 class="mt-10 mb-3 text-xs font-medium tracking-wide text-stone-500 uppercase dark:text-stone-400">{{ t.library.systemStatus }}</h2>
      <SystemStatus />
    </div>

    <template v-else>
      <div class="flex items-center gap-1 px-8 pt-4 pb-3">
        <button
          v-for="f in filters"
          :key="f.id"
          class="rounded-md px-2.5 py-1 text-stone-600 hover:bg-stone-200/70 dark:text-stone-400 dark:hover:bg-stone-800"
          :class="filter === f.id && '!bg-stone-200 !text-stone-900 dark:!bg-stone-800 dark:!text-stone-50'"
          @click="filter = f.id"
        >
          {{ t.library.filters[f.id] }} <span class="text-stone-400 tabular-nums">{{ formatInt(counts[f.id]) }}</span>
        </button>
        <button
          v-if="lib.unhandled.length"
          class="ml-auto rounded-md px-2.5 py-1 text-stone-500 hover:bg-stone-200/70 dark:hover:bg-stone-800"
          :title="t.library.unhandled.title"
          @click="showUnhandled = !showUnhandled"
        >
          {{ t.library.unhandled.count(formatInt(lib.unhandled.length)) }}
        </button>
      </div>

      <div v-if="showUnhandled" class="mx-8 mb-3 max-h-48 overflow-y-auto rounded-lg border border-stone-200 bg-white text-xs dark:border-stone-800 dark:bg-stone-900">
        <div v-for="f in lib.unhandled" :key="`${f.root_id}/${f.rel_path}`" class="flex gap-3 border-b border-stone-100 px-4 py-1.5 last:border-0 dark:border-stone-800">
          <span class="min-w-0 flex-1 truncate font-mono select-text" :title="`${lib.rootPath.get(f.root_id)}/${f.rel_path}`">{{ f.rel_path }}</span>
          <span class="text-stone-400">{{ f.kind === "sidecar" ? t.library.unhandled.sidecar : t.library.unhandled.unknown }}</span>
          <span class="w-20 text-right text-stone-400 tabular-nums">{{ formatBytes(f.size) }}</span>
        </div>
      </div>

      <div class="min-h-0 flex-1 px-8 pb-4">
        <p v-if="!visibleItems.length && !lib.scanning" class="pt-10 text-center text-stone-500">{{ t.library.noItems }}</p>
        <VirtualGrid v-else :items="visibleItems" :min-tile="156" :gap="12" :caption="34" :item-key="(i) => i.group_id">
          <template #default="{ item, size }">
            <GridTile :item="item" :size="size" />
          </template>
        </VirtualGrid>
      </div>
    </template>
    <PhotosImportDialog v-if="showPhotosImport" @close="showPhotosImport = false" @planned="onImportPlanned" />
  </section>
</template>
