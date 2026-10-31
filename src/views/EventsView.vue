<script setup lang="ts">
// Events (PLAN.md 6.2). Alle Änderungen betreffen nur die Datenbank.
import { computed, ref, watch } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { api, type EventCard, type EventsOverview, type TzMode } from "../api";
import { useLibraryStore } from "../stores/library";
import { formatInt, monthName } from "../format";
import { t } from "../i18n";
import EventCardRow from "../components/EventCardRow.vue";
import EventDetail from "../components/EventDetail.vue";

const lib = useLibraryStore();
const data = ref<EventsOverview | null>(null);
const error = ref<string | null>(null);
const selected = ref(new Set<number>());
const openId = ref<number | null>(null);
const showSettings = ref(false);
const busy = ref(false);

async function load() {
  try {
    data.value = await api.eventsOverview();
    error.value = null;
  } catch (e) {
    error.value = String(e);
  }
}
watch(() => lib.stats, load, { immediate: true });

const openEvent = computed(() => data.value?.events.find((e) => e.id === openId.value) ?? null);

/** Gruppiert nach Jahr und Monat des Event-Beginns. */
const timeline = computed(() => {
  const out: { key: string; label: string; events: EventCard[] }[] = [];
  for (const e of data.value?.events ?? []) {
    const key = e.date_start ? e.date_start.slice(0, 7) : "ohne";
    const label = e.date_start ? `${monthName(+key.slice(5, 7))} ${key.slice(0, 4)}` : t.common.noDateTitle;
    if (out.at(-1)?.key !== key) out.push({ key, label, events: [] });
    out.at(-1)!.events.push(e);
  }
  return out;
});

async function act(action: () => Promise<unknown>) {
  busy.value = true;
  try {
    await action();
    await load();
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}

function toggleSelect(id: number) {
  const next = new Set(selected.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  selected.value = next;
}

const merge = () =>
  act(async () => {
    await api.eventsMerge([...selected.value]);
    selected.value = new Set();
  });

async function saveSettings() {
  if (data.value) await api.eventsSaveSettings(data.value.settings);
}

async function recluster() {
  const ok = await ask(
    t.events.reclusterDialog.message,
    { title: t.events.reclusterDialog.title, kind: "warning", okLabel: t.events.reclusterDialog.ok, cancelLabel: t.common.cancel },
  );
  if (ok) await act(async () => {
    await saveSettings();
    await api.eventsRecluster();
  });
}

const setCamera = (camera: string, mode: TzMode | null) => act(() => api.cameraTzSet(camera, mode));
const rename = (id: number, title: string) => act(() => api.eventRename(id, title));
const confirm = (id: number, confirmed: boolean) => act(() => api.eventConfirm(id, confirmed));
</script>

<template>
  <section class="mx-auto max-w-5xl px-8 py-8">
    <header class="flex flex-wrap items-center gap-3">
      <h1 class="text-xl font-semibold tracking-tight">{{ t.events.title }}</h1>
      <span v-if="data" class="text-stone-500 tabular-nums dark:text-stone-400">
        {{ t.events.summary(formatInt(data.events.length), formatInt(data.events.filter((e) => e.confirmed).length)) }}
      </span>
      <button
        class="ml-auto rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800"
        @click="showSettings = !showSettings"
      >
        {{ t.events.settingsToggle }}
      </button>
    </header>
    <p class="mt-2 max-w-2xl text-stone-500 dark:text-stone-400">
      {{ t.events.intro }}
    </p>
    <p v-if="error" class="mt-3 rounded-md bg-red-50 px-3 py-2 text-red-700 dark:bg-red-500/10 dark:text-red-300">{{ error }}</p>

    <!-- Einstellungen -->
    <div v-if="showSettings && data" class="mt-4 space-y-4 rounded-lg border border-stone-200 bg-white p-4 dark:border-stone-800 dark:bg-stone-900">
      <div class="flex flex-wrap items-center gap-x-6 gap-y-3">
        <label class="flex items-center gap-2">
          {{ t.events.settings.gapBefore }}
          <input v-model.number="data.settings.gap_hours" type="number" min="0.25" max="72" step="0.5" class="w-16 rounded border border-stone-300 bg-transparent px-1.5 py-0.5 tabular-nums dark:border-stone-700" @change="saveSettings" />
          {{ t.events.settings.gapAfter }}
        </label>
        <label class="flex items-center gap-2">
          {{ t.events.settings.gpsBefore }}
          <input v-model.number="data.settings.gps_km" type="number" min="1" max="5000" step="5" class="w-16 rounded border border-stone-300 bg-transparent px-1.5 py-0.5 tabular-nums dark:border-stone-700" @change="saveSettings" />
          {{ t.events.settings.gpsAfter }}
        </label>
        <label class="flex items-center gap-2">
          <input v-model="data.settings.merge_multiday" type="checkbox" class="accent-stone-800 dark:accent-stone-200" @change="saveSettings" />
          {{ t.events.settings.mergeMultiday }}
        </label>
        <button
          :disabled="busy"
          class="ml-auto rounded-md bg-stone-900 px-3 py-1.5 text-white hover:bg-stone-700 disabled:opacity-40 dark:bg-stone-100 dark:text-stone-900"
          @click="recluster"
        >
          {{ t.events.settings.recluster }}
        </button>
      </div>

      <div v-if="data.cameras.length" class="border-t border-stone-100 pt-3 dark:border-stone-800">
        <h3 class="font-medium">{{ t.events.settings.videoTzTitle }}</h3>
        <p class="mt-0.5 text-xs text-stone-500">
          {{ t.events.settings.videoTzHint }}
        </p>
        <ul class="mt-2 space-y-1.5">
          <li v-for="c in data.cameras" :key="c.camera" class="flex items-center gap-3">
            <span class="min-w-0 flex-1 truncate">{{ c.camera }} <span class="text-stone-400">· {{ t.events.settings.videos(c.video_count) }}</span></span>
            <div class="flex overflow-hidden rounded-md border border-stone-300 text-xs dark:border-stone-700">
              <button
                v-for="m in ['local', 'utc'] as const"
                :key="m"
                class="px-2.5 py-1"
                :class="c.mode === m ? 'bg-stone-900 text-white dark:bg-stone-100 dark:text-stone-900' : 'hover:bg-stone-100 dark:hover:bg-stone-800'"
                @click="setCamera(c.camera, m)"
              >
                {{ m === "local" ? t.events.settings.localTime : t.events.settings.utc }}
              </button>
            </div>
            <span class="w-20 text-xs text-stone-400">
              <template v-if="c.is_default">{{ t.events.settings.isDefault }}</template>
              <button v-else class="underline-offset-2 hover:underline" @click="setCamera(c.camera, null)">{{ t.events.settings.reset }}</button>
            </span>
          </li>
        </ul>
      </div>
    </div>

    <!-- Auswahl -->
    <div
      v-if="selected.size"
      class="sticky top-0 z-10 mt-4 flex items-center gap-3 rounded-lg border border-sky-200 bg-sky-50 px-4 py-2 dark:border-sky-900 dark:bg-sky-950"
    >
      <span>{{ t.events.selection.count(selected.size) }}</span>
      <button
        :disabled="selected.size < 2 || busy"
        class="rounded-md bg-stone-900 px-3 py-1 text-white hover:bg-stone-700 disabled:opacity-40 dark:bg-stone-100 dark:text-stone-900"
        @click="merge"
      >
        {{ t.events.selection.merge }}
      </button>
      <button class="text-stone-600 hover:text-stone-900 dark:text-stone-400 dark:hover:text-stone-100" @click="selected = new Set()">{{ t.events.selection.clear }}</button>
    </div>

    <!-- Timeline -->
    <div v-if="data && !data.events.length" class="mt-6 rounded-lg border border-dashed border-stone-300 px-6 py-10 text-center text-stone-500 dark:border-stone-700">
      {{ t.events.empty }}
    </div>
    <div v-for="month in timeline" :key="month.key" class="mt-6">
      <h2 class="mb-2 text-xs font-medium tracking-wide text-stone-500 uppercase dark:text-stone-400">{{ month.label }}</h2>
      <div class="space-y-2">
        <EventCardRow
          v-for="e in month.events"
          :key="e.id"
          :event="e"
          :selected="selected.has(e.id)"
          @toggle-select="toggleSelect(e.id)"
          @open="openId = e.id"
          @rename="(title) => rename(e.id, title)"
          @confirm="(c) => confirm(e.id, c)"
        />
      </div>
    </div>
    <p v-if="data && data.unassigned" class="mt-6 text-xs text-stone-400">
      {{ t.events.unassigned(formatInt(data.unassigned)) }}
    </p>

    <EventDetail v-if="openEvent && data" :event="openEvent" :events="data.events" @close="openId = null" @changed="load" />
  </section>
</template>
