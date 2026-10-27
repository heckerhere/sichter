<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { ask } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import SystemStatus from "../components/SystemStatus.vue";
import { useAppStore } from "../stores/app";
import { formatBytes } from "../format";

const app = useAppStore();
const router = useRouter();
const cache = ref<{ data_dir: string; thumbs: number; previews: number; logs: number } | null>(null);
const error = ref<string | null>(null);

const xmp = ref(false);
const loadCache = async () => (cache.value = await api.cacheInfo());
onMounted(async () => {
  await loadCache();
  xmp.value = await api.xmpExportEnabled();
});

async function toggleXmp() {
  await api.setXmpExportEnabled(!xmp.value);
  xmp.value = !xmp.value;
}

async function toggleDryRun() {
  if (app.dryRun) {
    const ok = await ask("Danach kannst du Pläne wirklich ausführen. Jede Ausführung fragt weiterhin mit Dateianzahl nach.", {
      title: "Dry-Run-Modus ausschalten",
      okLabel: "Ausschalten",
      cancelLabel: "Abbrechen",
    });
    if (!ok) return;
  }
  await app.setDryRun(!app.dryRun);
}

async function clear(which: "thumbs" | "previews") {
  const ok = await ask(
    which === "thumbs"
      ? "Alle Vorschaubilder werden gelöscht und beim nächsten Scan neu erzeugt. Deine Fotos sind davon nicht betroffen."
      : "Die großen Vorschauen für das Sichten werden gelöscht und bei Bedarf neu erzeugt. Deine Fotos sind davon nicht betroffen.",
    { title: "Cache leeren", okLabel: "Leeren", cancelLabel: "Abbrechen" },
  );
  if (!ok) return;
  await api.cacheClear(which);
  await loadCache();
}

async function emptyDirs() {
  try {
    await router.push(`/plan/${await api.planEmptyDirs()}`);
  } catch (e) {
    error.value = String(e);
  }
}

const openDataDir = () => api.openDataDir().catch((e) => (error.value = String(e)));
</script>

<template>
  <section class="mx-auto max-w-3xl px-8 py-10">
    <h1 class="text-xl font-semibold tracking-tight">Einstellungen</h1>
    <p v-if="error" class="mt-3 rounded-md bg-red-50 px-3 py-2 text-red-700 dark:bg-red-500/10 dark:text-red-300">{{ error }}</p>

    <div class="mt-6 flex items-center gap-4 rounded-lg border border-stone-200 bg-white p-4 dark:border-stone-800 dark:bg-stone-900">
      <div class="flex-1">
        <h2 class="font-medium">Dry-Run-Modus</h2>
        <p class="mt-0.5 text-stone-500 dark:text-stone-400">
          Solange er an ist, lassen sich Pläne nur als Probelauf durchspielen. Es wird keine Datei verändert. Standard ist an.
        </p>
      </div>
      <button
        class="rounded-md px-3 py-1.5"
        :class="app.dryRun ? 'bg-amber-100 font-medium text-amber-800 dark:bg-amber-500/15 dark:text-amber-300' : 'border border-stone-300 dark:border-stone-700'"
        @click="toggleDryRun"
      >
        {{ app.dryRun ? "An" : "Aus" }}
      </button>
    </div>

    <div class="mt-3 flex items-center gap-4 rounded-lg border border-stone-200 bg-white p-4 dark:border-stone-800 dark:bg-stone-900">
      <div class="flex-1">
        <h2 class="font-medium">XMP-Export der Bewertungen</h2>
        <p class="mt-0.5 text-stone-500 dark:text-stone-400">
          Schreibt Sterne und Ausschuss (Bewertung -1) in XMP-Sidecars neben RAW-Dateien, damit Lightroom, Capture One und Bridge sie sehen. Nie in die Bilddatei
          selbst. Bestehende XMP-Dateien werden ergänzt und vorher gesichert, alles läuft über einen Plan und lässt sich rückgängig machen. Standard ist aus.
        </p>
      </div>
      <button
        class="rounded-md px-3 py-1.5"
        :class="xmp ? 'bg-stone-900 text-white dark:bg-stone-100 dark:text-stone-900' : 'border border-stone-300 dark:border-stone-700'"
        @click="toggleXmp"
      >
        {{ xmp ? "An" : "Aus" }}
      </button>
    </div>

    <h2 class="mt-8 mb-3 text-xs font-medium tracking-wide text-stone-500 uppercase dark:text-stone-400">Wartung</h2>
    <div class="divide-y divide-stone-100 rounded-lg border border-stone-200 bg-white dark:divide-stone-800 dark:border-stone-800 dark:bg-stone-900">
      <div class="flex items-center gap-4 p-4">
        <div class="flex-1">
          <h3 class="font-medium">Leere Ordner bereinigen</h3>
          <p class="mt-0.5 text-stone-500 dark:text-stone-400">
            Findet Ordner, die nichts außer .DS_Store oder Thumbs.db enthalten, etwa nach dem Umsortieren. Du siehst den Plan und bestätigst, die Ordner kommen in den
            Papierkorb. Gesperrte Ordner werden nicht durchsucht.
          </p>
        </div>
        <button class="shrink-0 rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="emptyDirs">Suchen …</button>
      </div>
      <div v-if="cache" class="flex items-center gap-4 p-4">
        <div class="flex-1">
          <h3 class="font-medium">Vorschaubilder</h3>
          <p class="mt-0.5 text-stone-500 tabular-nums dark:text-stone-400">Grid {{ formatBytes(cache.thumbs) }} · Sichten {{ formatBytes(cache.previews) }} · Protokolle {{ formatBytes(cache.logs) }}</p>
        </div>
        <button class="shrink-0 rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="clear('previews')">Sichten-Cache leeren</button>
        <button class="shrink-0 rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="clear('thumbs')">Grid-Cache leeren</button>
      </div>
      <div v-if="cache" class="flex items-center gap-4 p-4">
        <div class="min-w-0 flex-1">
          <h3 class="font-medium">Datenordner</h3>
          <p class="mt-0.5 truncate font-mono text-[11px] text-stone-500 select-text" :title="cache.data_dir">{{ cache.data_dir }}</p>
        </div>
        <button class="shrink-0 rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="openDataDir">Im Finder zeigen</button>
      </div>
    </div>

    <h2 class="mt-8 mb-3 text-xs font-medium tracking-wide text-stone-500 uppercase dark:text-stone-400">Systemstatus</h2>
    <SystemStatus />
    <p class="mt-4 text-xs text-stone-400">
      Ortsnamen: GeoNames (geonames.org), Lizenz CC BY 4.0. Metadaten: ExifTool von Phil Harvey. Video und HEIC: FFmpeg (LGPL 2.1).
    </p>
  </section>
</template>
