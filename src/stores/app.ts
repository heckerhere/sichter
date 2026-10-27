import { defineStore } from "pinia";
import { ref } from "vue";
import { api, type AppInfo } from "../api";

export const useAppStore = defineStore("app", () => {
  const info = ref<AppInfo | null>(null);
  const error = ref<string | null>(null);
  // Sicherheitsmodell 3.3: Dry-Run ist der Standard.
  const dryRun = ref(true);

  async function load() {
    try {
      info.value = await api.appInfo();
      error.value = null;
    } catch (e) {
      error.value = String(e);
    }
  }

  return { info, error, dryRun, load };
});
