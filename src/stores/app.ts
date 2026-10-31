import { defineStore } from "pinia";
import { ref } from "vue";
import { api, type AppInfo } from "../api";
import { locale, resolveLocale, type LanguagePref } from "../i18n";

export const useAppStore = defineStore("app", () => {
  const info = ref<AppInfo | null>(null);
  const error = ref<string | null>(null);
  // Sicherheitsmodell 3.3: Dry-Run ist der Standard. Der Wert kommt aus der
  // Datenbank; bis er geladen ist, gilt sicherheitshalber "an".
  const dryRun = ref(true);

  async function loadDryRun() {
    dryRun.value = await api.runsDryRunMode();
  }

  async function setDryRun(enabled: boolean) {
    await api.runsSetDryRunMode(enabled);
    dryRun.value = enabled;
  }

  // Sprache: "auto" folgt dem System. Die tatsächliche Sprache geht auch ans
  // Backend (Meldungen, Ordnernamen, die Sichter anlegt).
  const language = ref<LanguagePref>("auto");

  async function applyLanguage(pref: LanguagePref) {
    const effective = resolveLocale(pref);
    await api.setUiLanguage(pref, effective);
    language.value = pref;
    locale.value = effective;
  }

  async function loadLanguage() {
    try {
      const pref = (await api.uiLanguage()) as LanguagePref;
      await applyLanguage(["auto", "de", "en"].includes(pref) ? pref : "auto");
    } catch (e) {
      error.value = String(e);
    }
  }

  async function load() {
    try {
      info.value = await api.appInfo();
      error.value = null;
    } catch (e) {
      error.value = String(e);
    }
  }

  return { info, error, dryRun, language, load, loadDryRun, setDryRun, loadLanguage, applyLanguage };
});
