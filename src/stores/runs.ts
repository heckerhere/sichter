import { defineStore } from "pinia";
import { ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { api, type RunFinished, type RunProgress, type RunState } from "../api";
import { useAppStore } from "./app";
import { useLibraryStore } from "./library";

export const useRunsStore = defineStore("runs", () => {
  const state = ref<RunState | null>(null);
  const progress = ref<RunProgress | null>(null);
  const lastFinished = ref<RunFinished | null>(null);

  async function refresh() {
    state.value = await api.runState();
  }

  async function init() {
    await listen<RunProgress>("run-progress", (e) => (progress.value = e.payload));
    await listen<RunFinished>("run-finished", async (e) => {
      progress.value = null;
      lastFinished.value = e.payload;
      await refresh();
      // Dateien haben sich bewegt: Bibliothek, Speicher usw. neu laden.
      await useLibraryStore().refresh();
    });
    await refresh();
    await useAppStore().loadDryRun();
  }

  return { state, progress, lastFinished, refresh, init };
});
