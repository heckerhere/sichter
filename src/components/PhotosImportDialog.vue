<script setup lang="ts">
// Import aus der Fotos-Mediathek (macOS): Zielordner wählen, Zugriff erfragen,
// Plan erstellen. Ausgeführt wird wie jeder Plan erst nach Vorschau und Bestätigung.
import { computed, onMounted, onUnmounted, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api, type PhotosAccess } from "../api";
import { formatInt } from "../format";
import { t } from "../i18n";

const emit = defineEmits<{ close: []; planned: [runId: number] }>();

const access = ref<PhotosAccess | null>(null);
const target = ref<string | null>(null);
const downloadOriginals = ref(true);
const includeHidden = ref(false);
const busy = ref(false);
const error = ref<string | null>(null);

// Fortschritt beim Einlesen der Mediathek.
const progress = ref<{ done: number; total: number } | null>(null);
let unlisten: UnlistenFn | null = null;
onMounted(async () => {
  unlisten = await listen<{ done: number; total: number }>("photos-progress", (e) => (progress.value = e.payload));
  access.value = (await api.photosStatus()).access;
});
onUnmounted(() => unlisten?.());
const percent = computed(() => (progress.value && progress.value.total ? Math.round((progress.value.done / progress.value.total) * 100) : 0));
const reading = computed(() => !!progress.value && progress.value.done < progress.value.total);

const blocked = computed(() => access.value === "denied" || access.value === "restricted");

async function pickTarget() {
  const picked = await open({ directory: true, multiple: false, title: t.library.photos.pickTargetTitle });
  if (typeof picked === "string") target.value = picked;
}

async function plan() {
  if (!target.value) return;
  error.value = null;
  progress.value = null;
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
      <h2 class="text-lg font-semibold">{{ t.library.photos.title }}</h2>
      <p class="mt-1 text-stone-500 dark:text-stone-400">
        {{ t.library.photos.introBefore }}
        <span class="font-mono text-xs">{{ t.library.photos.introPattern }}</span>{{ t.library.photos.introAfter }}
      </p>

      <div v-if="blocked" class="mt-4 rounded-lg border border-amber-200 bg-amber-50 px-4 py-3 text-amber-900 dark:border-amber-900 dark:bg-amber-950 dark:text-amber-200">
        {{ t.library.photos.blocked }}
        <button class="mt-2 block rounded-md border border-amber-300 px-2.5 py-1 text-xs hover:bg-amber-100 dark:border-amber-800 dark:hover:bg-amber-900" @click="api.photosOpenPrivacySettings()">
          {{ t.library.photos.openSettings }}
        </button>
      </div>
      <p v-else-if="access === 'limited'" class="mt-4 rounded-md bg-stone-100 px-3 py-2 text-xs text-stone-600 dark:bg-stone-800 dark:text-stone-300">
        {{ t.library.photos.limited }}
      </p>

      <div class="mt-4 flex items-center gap-3">
        <button class="rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="pickTarget">{{ t.library.photos.pickTarget }}</button>
        <span class="min-w-0 flex-1 truncate font-mono text-xs" :class="target ? '' : 'text-stone-400'" :title="target ?? ''">{{ target ?? t.library.photos.noTarget }}</span>
      </div>

      <label class="mt-4 flex items-start gap-2">
        <input v-model="downloadOriginals" type="checkbox" class="mt-0.5 size-4" />
        <span>
          {{ t.library.photos.downloadOriginals }}
          <span class="block text-xs text-stone-500">{{ t.library.photos.downloadOriginalsHint }}</span>
        </span>
      </label>
      <label class="mt-2 flex items-start gap-2">
        <input v-model="includeHidden" type="checkbox" class="mt-0.5 size-4" />
        <span>{{ t.library.photos.includeHidden }}</span>
      </label>

      <p v-if="error" class="mt-3 rounded-md bg-red-50 px-3 py-2 text-red-700 dark:bg-red-500/10 dark:text-red-300">{{ error }}</p>

      <div v-if="busy" class="mt-4">
        <div class="flex items-baseline gap-2 text-xs text-stone-500">
          <span v-if="!progress">{{ t.library.photos.waiting }}</span>
          <template v-else-if="reading">
            <span>{{ t.library.photos.reading }}</span>
            <span class="tabular-nums">{{ t.library.photos.objects(formatInt(progress.done), formatInt(progress.total)) }}</span>
            <span class="ml-auto font-medium tabular-nums text-stone-700 dark:text-stone-300">{{ t.library.photos.percent(percent) }}</span>
          </template>
          <span v-else>{{ t.library.photos.planning(formatInt(progress.total)) }}</span>
        </div>
        <div class="mt-1.5 h-1.5 overflow-hidden rounded-full bg-stone-100 dark:bg-stone-800">
          <div
            class="h-full rounded-full bg-sky-500 transition-[width] duration-300"
            :class="!progress && 'animate-pulse'"
            :style="{ width: `${progress ? Math.max(2, percent) : 100}%` }"
          />
        </div>
      </div>

      <div class="mt-5 flex items-center justify-end gap-2">
        <button class="rounded-md border border-stone-300 px-3 py-1.5 dark:border-stone-700" @click="emit('close')">{{ t.common.cancel }}</button>
        <button
          :disabled="!target || busy || blocked"
          class="rounded-md bg-stone-900 px-3 py-1.5 text-white hover:bg-stone-700 disabled:opacity-40 dark:bg-stone-100 dark:text-stone-900"
          @click="plan"
        >
          {{ t.library.photos.createPlan }}
        </button>
      </div>
    </div>
  </div>
</template>
