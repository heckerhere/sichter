<script setup lang="ts">
// Sichten: Event wählen, dann Vollbild-Sichtung.
import { computed, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { api, type EventsOverview } from "../api";
import { useLibraryStore } from "../stores/library";
import { formatInt, formatRange } from "../format";
import CullLoupe from "../components/CullLoupe.vue";
import RejectsTrashDialog from "../components/RejectsTrashDialog.vue";

const lib = useLibraryStore();
const router = useRouter();
const showTrash = ref(false);
const error = ref<string | null>(null);
const xmpEnabled = ref(false);
void api.xmpExportEnabled().then((v) => (xmpEnabled.value = v));

async function exportXmp() {
  try {
    await router.push(`/plan/${await api.planXmpExport()}`);
  } catch (e) {
    error.value = String(e);
  }
}

async function rejectsToFolder() {
  try {
    await router.push(`/plan/${await api.planRejects(false, [])}`);
  } catch (e) {
    error.value = String(e);
  }
}
const data = ref<EventsOverview | null>(null);
const session = ref<{ eventId: number | null; title: string } | null>(null);

async function load() {
  data.value = await api.eventsOverview();
}
watch(() => lib.stats, load, { immediate: true });

const totals = computed(() => {
  const events = data.value?.events ?? [];
  return {
    count: events.reduce((s, e) => s + e.count, 0),
    rated: events.reduce((s, e) => s + e.rated_count, 0),
    keep: events.reduce((s, e) => s + e.keep_count, 0),
    reject: events.reduce((s, e) => s + e.reject_count, 0),
  };
});

const label = (e: { title: string; suggested_title: string; date_start: string | null; date_end: string | null }) =>
  e.title || e.suggested_title || formatRange(e.date_start, e.date_end);

async function close() {
  session.value = null;
  await load();
  await lib.refresh();
}
</script>

<template>
  <section class="mx-auto max-w-4xl px-8 py-8">
    <h1 class="text-xl font-semibold tracking-tight">Sichten</h1>
    <p class="mt-2 max-w-2xl text-stone-500 dark:text-stone-400">
      Bewerte Aufnahmen im Vollbild mit der Tastatur. Ausschuss bleibt liegen, bis du ihn in einem eigenen Schritt verschiebst. Bewertungen speichert nur Sichter,
      die Dateien bleiben unverändert.
    </p>

    <div v-if="totals.reject" class="mt-6 flex flex-wrap items-center gap-3 rounded-lg border border-stone-200 bg-white px-4 py-3 dark:border-stone-800 dark:bg-stone-900">
      <span class="flex-1"><span class="font-medium">{{ totals.reject }} Aufnahmen als Ausschuss markiert.</span> Sie liegen noch an ihrem Ort.</span>
      <button class="rounded-md bg-stone-900 px-3 py-1.5 text-white hover:bg-stone-700 dark:bg-stone-100 dark:text-stone-900" @click="rejectsToFolder">
        Nach _Aussortiert verschieben …
      </button>
      <button class="rounded-md border border-red-300 px-3 py-1.5 text-red-700 hover:bg-red-50 dark:border-red-900 dark:text-red-400 dark:hover:bg-red-950" @click="showTrash = true">
        In den Papierkorb …
      </button>
    </div>
    <div v-if="xmpEnabled && totals.rated" class="mt-3 flex items-center gap-3 rounded-lg border border-stone-200 bg-white px-4 py-3 dark:border-stone-800 dark:bg-stone-900">
      <span class="flex-1">Bewertungen von RAW-Aufnahmen für Lightroom & Co. als XMP-Sidecar speichern.</span>
      <button class="rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="exportXmp">Als XMP exportieren …</button>
    </div>
    <p v-if="error" class="mt-3 rounded-md bg-red-50 px-3 py-2 text-red-700 dark:bg-red-500/10 dark:text-red-300">{{ error }}</p>

    <button
      v-if="totals.count"
      class="mt-6 flex w-full items-center gap-4 rounded-lg border border-stone-200 bg-white px-4 py-3 text-left hover:border-stone-300 dark:border-stone-800 dark:bg-stone-900 dark:hover:border-stone-700"
      @click="session = { eventId: null, title: 'Alle Aufnahmen' }"
    >
      <span class="flex-1 font-medium">Alle Aufnahmen</span>
      <span class="text-xs text-stone-500 tabular-nums">{{ formatInt(totals.rated) }} von {{ formatInt(totals.count) }} bewertet · {{ totals.keep }} behalten · {{ totals.reject }} Ausschuss</span>
    </button>

    <ul class="mt-3 space-y-1.5">
      <li v-for="e in data?.events ?? []" :key="e.id">
        <button
          class="flex w-full items-center gap-4 rounded-lg border border-stone-200 bg-white px-4 py-2.5 text-left hover:border-stone-300 dark:border-stone-800 dark:bg-stone-900 dark:hover:border-stone-700"
          @click="session = { eventId: e.id, title: label(e) }"
        >
          <div class="min-w-0 flex-1">
            <div class="truncate font-medium">{{ label(e) }}</div>
            <div class="text-xs text-stone-500 tabular-nums">{{ formatRange(e.date_start, e.date_end) }} · {{ formatInt(e.count) }} Aufnahmen</div>
          </div>
          <div class="w-48">
            <div class="flex h-1.5 gap-[2px] overflow-hidden rounded-full" style="background: var(--viz-track)" :title="`${e.keep_count} behalten, ${e.reject_count} Ausschuss, ${e.rated_count} bewertet`">
              <div class="h-full bg-emerald-500" :style="{ width: `${(e.keep_count / e.count) * 100}%` }" />
              <div class="h-full bg-red-500" :style="{ width: `${(e.reject_count / e.count) * 100}%` }" />
              <div class="h-full bg-stone-400" :style="{ width: `${((e.rated_count - e.keep_count - e.reject_count) / e.count) * 100}%` }" />
            </div>
            <div class="mt-1 text-right text-[11px] text-stone-400 tabular-nums">{{ e.rated_count }} / {{ e.count }} bewertet</div>
          </div>
        </button>
      </li>
    </ul>
    <p v-if="data && !data.events.length" class="mt-6 rounded-lg border border-dashed border-stone-300 px-6 py-10 text-center text-stone-500 dark:border-stone-700">
      Noch keine Aufnahmen. Füge in der Bibliothek einen Ordner hinzu.
    </p>

    <RejectsTrashDialog v-if="showTrash" @close="showTrash = false" @planned="(id) => router.push(`/plan/${id}`)" />
    <CullLoupe v-if="session" :event-id="session.eventId" :title="session.title" @close="close" />
  </section>
</template>
