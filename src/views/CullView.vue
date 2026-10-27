<script setup lang="ts">
// Sichten: Event wählen, dann Vollbild-Sichtung.
import { computed, ref, watch } from "vue";
import { api, type EventsOverview } from "../api";
import { useLibraryStore } from "../stores/library";
import { formatInt, formatRange } from "../format";
import CullLoupe from "../components/CullLoupe.vue";

const lib = useLibraryStore();
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

    <CullLoupe v-if="session" :event-id="session.eventId" :title="session.title" @close="close" />
  </section>
</template>
