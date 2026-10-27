import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  type GridItem,
  type LibraryStats,
  type RootInfo,
  type ScanFinished,
  type ScanProgress,
  type ScanReport,
  type UnhandledFile,
} from "../api";

export const useLibraryStore = defineStore("library", () => {
  const roots = ref<RootInfo[]>([]);
  const groups = ref<GridItem[]>([]);
  const unhandled = ref<UnhandledFile[]>([]);
  const stats = ref<LibraryStats | null>(null);
  const scanning = ref(false);
  const progress = ref<ScanProgress | null>(null);
  const lastReport = ref<ScanReport | null>(null);
  const error = ref<string | null>(null);

  const rootPath = computed(() => new Map(roots.value.map((r) => [r.id, r.path])));

  async function refresh() {
    try {
      [roots.value, groups.value, unhandled.value, stats.value] = await Promise.all([
        api.listRoots(),
        api.listGroups(),
        api.listUnhandled(),
        api.libraryStats(),
      ]);
    } catch (e) {
      error.value = String(e);
    }
  }

  // Während des Scans das Grid nachladen, damit Kacheln und Vorschaubilder
  // schon beim Entstehen sichtbar werden.
  let lastRefresh = 0;
  function refreshDuringScan(p: ScanProgress) {
    const phaseChanged = p.phase !== progress.value?.phase;
    const live = p.phase === "hash" || p.phase === "thumbs";
    if (phaseChanged || (live && Date.now() - lastRefresh > 2500)) {
      lastRefresh = Date.now();
      void refresh();
    }
  }

  async function init() {
    await listen<ScanProgress>("scan-progress", (e) => {
      refreshDuringScan(e.payload);
      progress.value = e.payload;
    });
    await listen<ScanFinished>("scan-finished", async (e) => {
      scanning.value = false;
      progress.value = null;
      if (e.payload.status === "ok") lastReport.value = e.payload.report;
      else error.value = e.payload.message;
      await refresh();
    });
    scanning.value = await api.scanRunning();
    await refresh();
  }

  async function addRoots(paths: string[]): Promise<string[]> {
    const results = await api.addRoots(paths);
    await refresh();
    const added = results.filter((r) => r.id !== null).map((r) => r.id!);
    if (added.length) await startScan(added);
    return results.filter((r) => r.error).map((r) => `${r.path}: ${r.error}`);
  }

  async function removeRoot(id: number) {
    await api.removeRoot(id);
    await refresh();
  }

  async function startScan(rootIds?: number[]) {
    error.value = null;
    try {
      await api.startScan(rootIds);
      scanning.value = true;
      progress.value = { phase: "crawl", done: 0, total: 0, current: null };
    } catch (e) {
      error.value = String(e);
    }
  }

  const cancelScan = () => api.cancelScan();

  return { roots, groups, unhandled, stats, scanning, progress, lastReport, error, rootPath, init, refresh, addRoots, removeRoot, startScan, cancelScan };
});
