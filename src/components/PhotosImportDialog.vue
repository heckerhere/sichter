<script setup lang="ts">
// Import aus der Fotos-Mediathek (macOS): Zielordner wählen, Zugriff erfragen,
// Plan erstellen. Ausgeführt wird wie jeder Plan erst nach Vorschau und Bestätigung.
import { computed, onMounted, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type PhotosAccess } from "../api";

const emit = defineEmits<{ close: []; planned: [runId: number] }>();

const access = ref<PhotosAccess | null>(null);
const target = ref<string | null>(null);
const downloadOriginals = ref(true);
const includeHidden = ref(false);
const busy = ref(false);
const error = ref<string | null>(null);

onMounted(async () => (access.value = (await api.photosStatus()).access));

const blocked = computed(() => access.value === "denied" || access.value === "restricted");

async function pickTarget() {
  const picked = await open({ directory: true, multiple: false, title: "Zielordner für den Import" });
  if (typeof picked === "string") target.value = picked;
}

async function plan() {
  if (!target.value) return;
  error.value = null;
  busy.value = true;
  try {
    if (access.value === "not_determined") access.value = await api.photosRequestAccess();
    if (access.value !== "authorized" && access.value !== "limited") return;
    emit("planned", await api.planPhotosImport({ target: target.value, include_hidden: includeHidden.value, download_originals: downloadOriginals.value }));
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-8">
    <div class="w-full max-w-xl rounded-xl bg-white p-5 shadow-xl dark:bg-stone-900">
      <h2 class="text-lg font-semibold">Aus der Fotos-Mediathek importieren</h2>
      <p class="mt-1 text-stone-500 dark:text-stone-400">
        Sichter kopiert die Originale (inklusive RAW und Live-Photo-Videos) in einen Ordner, sortiert nach
        <span class="font-mono text-xs">Jahr/Datum</span>. Die Mediathek bleibt unverändert, Bearbeitungen aus Fotos bleiben in Fotos. Der Ordner wird danach
        zur Bibliothek hinzugefügt und gescannt.
      </p>

      <div v-if="blocked" class="mt-4 rounded-lg border border-amber-200 bg-amber-50 px-4 py-3 text-amber-900 dark:border-amber-900 dark:bg-amber-950 dark:text-amber-200">
        Sichter hat keinen Zugriff auf die Fotos-Mediathek. Erlaube ihn unter Systemeinstellungen → Datenschutz &amp; Sicherheit → Fotos und starte Sichter danach neu.
        <button class="mt-2 block rounded-md border border-amber-300 px-2.5 py-1 text-xs hover:bg-amber-100 dark:border-amber-800 dark:hover:bg-amber-900" @click="api.photosOpenPrivacySettings()">
          Systemeinstellungen öffnen
        </button>
      </div>
      <p v-else-if="access === 'limited'" class="mt-4 rounded-md bg-stone-100 px-3 py-2 text-xs text-stone-600 dark:bg-stone-800 dark:text-stone-300">
        Du hast nur ausgewählte Fotos freigegeben. Es werden nur diese importiert.
      </p>

      <div class="mt-4 flex items-center gap-3">
        <button class="rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="pickTarget">Zielordner wählen …</button>
        <span class="min-w-0 flex-1 truncate font-mono text-xs" :class="target ? '' : 'text-stone-400'" :title="target ?? ''">{{ target ?? "noch keiner gewählt" }}</span>
      </div>

      <label class="mt-4 flex items-start gap-2">
        <input v-model="downloadOriginals" type="checkbox" class="mt-0.5 size-4" />
        <span>
          Originale aus iCloud laden
          <span class="block text-xs text-stone-500">Bei „Mac-Speicher optimieren“ liegen viele Originale nur in iCloud. Ohne diese Option werden sie übersprungen.</span>
        </span>
      </label>
      <label class="mt-2 flex items-start gap-2">
        <input v-model="includeHidden" type="checkbox" class="mt-0.5 size-4" />
        <span>Ausgeblendete Fotos mit importieren</span>
      </label>

      <p v-if="error" class="mt-3 rounded-md bg-red-50 px-3 py-2 text-red-700 dark:bg-red-500/10 dark:text-red-300">{{ error }}</p>

      <div class="mt-5 flex items-center justify-end gap-2">
        <span v-if="busy" class="mr-auto text-xs text-stone-500">Mediathek wird gelesen …</span>
        <button class="rounded-md border border-stone-300 px-3 py-1.5 dark:border-stone-700" @click="emit('close')">Abbrechen</button>
        <button
          :disabled="!target || busy || blocked"
          class="rounded-md bg-stone-900 px-3 py-1.5 text-white hover:bg-stone-700 disabled:opacity-40 dark:bg-stone-100 dark:text-stone-900"
          @click="plan"
        >
          Plan erstellen
        </button>
      </div>
    </div>
  </div>
</template>
