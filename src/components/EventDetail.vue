<script setup lang="ts">
// Detailansicht eines Events: teilen, Aufnahmen verschieben, Notizen.
import { computed, onMounted, ref, watch } from "vue";
import { api, type EventCard, type EventItem, type GridItem } from "../api";
import { formatInt, formatRange } from "../format";
import VirtualGrid from "./VirtualGrid.vue";
import GridTile from "./GridTile.vue";

const props = defineProps<{ event: EventCard; events: EventCard[] }>();
const emit = defineEmits<{ close: []; changed: [] }>();

const items = ref<EventItem[]>([]);
const selected = ref(new Set<number>());
const notes = ref(props.event.notes);
const error = ref<string | null>(null);

async function load() {
  items.value = await api.eventItems(props.event.id);
  selected.value = new Set();
}
onMounted(load);
watch(() => props.event.id, load);

const tiles = computed<GridItem[]>(() =>
  items.value.map((i) => ({
    group_id: i.group_id,
    asset_id: i.group_id,
    root_id: i.root_id,
    rel_path: i.rel_path,
    kind: i.kind,
    kinds: i.kinds,
    thumb: i.thumb,
    taken_at: i.time,
    date_source: i.date_source,
    duration_s: i.duration_s,
    ext_mismatch: false,
  })),
);

const index = computed(() => props.events.findIndex((e) => e.id === props.event.id));
const neighbours = computed(() =>
  [
    { label: "Vorheriges Event", event: props.events[index.value - 1] },
    { label: "Nächstes Event", event: props.events[index.value + 1] },
  ].filter((n) => n.event),
);
const label = (e: EventCard) => e.title || e.suggested_title || formatRange(e.date_start, e.date_end);

function toggle(id: number) {
  const next = new Set(selected.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  selected.value = next;
}

const splitTarget = computed(() => {
  if (selected.value.size !== 1) return null;
  const id = [...selected.value][0];
  return items.value.findIndex((i) => i.group_id === id) > 0 ? id : null;
});

async function run(action: () => Promise<unknown>) {
  try {
    await action();
    error.value = null;
    emit("changed");
    await load();
  } catch (e) {
    error.value = String(e);
  }
}

const split = () => run(() => api.eventSplit(props.event.id, splitTarget.value!));
const moveTo = (target: number | null) => run(() => api.eventsMove([...selected.value], target));
const saveNotes = () => notes.value !== props.event.notes && run(() => api.eventSetNotes(props.event.id, notes.value));
</script>

<template>
  <div class="fixed inset-0 z-20 flex flex-col bg-stone-50 dark:bg-stone-950">
    <header class="flex items-center gap-4 border-b border-stone-200 px-6 pt-8 pb-3 dark:border-stone-800" data-tauri-drag-region>
      <button class="rounded-md px-2 py-1 text-stone-600 hover:bg-stone-200/70 dark:text-stone-400 dark:hover:bg-stone-800" @click="emit('close')">← Zurück</button>
      <div class="min-w-0">
        <h2 class="truncate text-lg font-semibold tracking-tight">{{ label(event) }}</h2>
        <p class="text-xs text-stone-500 tabular-nums">{{ formatRange(event.date_start, event.date_end) }} · {{ formatInt(items.length) }} Aufnahmen</p>
      </div>
      <input
        v-model="notes"
        class="ml-auto w-72 rounded-md border border-stone-200 bg-white px-2.5 py-1 text-xs dark:border-stone-700 dark:bg-stone-900"
        placeholder="Notiz zum Event"
        @blur="saveNotes"
      />
    </header>

    <div class="flex flex-wrap items-center gap-2 px-6 py-2.5 text-xs">
      <span class="text-stone-500">{{ selected.size ? `${selected.size} ausgewählt` : "Aufnahmen anklicken, um sie auszuwählen" }}</span>
      <button
        :disabled="!splitTarget"
        class="rounded-md border border-stone-300 px-2.5 py-1 hover:bg-stone-100 disabled:opacity-40 dark:border-stone-700 dark:hover:bg-stone-800"
        title="Die gewählte Aufnahme und alle späteren werden ein eigenes Event"
        @click="split"
      >
        Ab hier teilen
      </button>
      <template v-if="selected.size">
        <span class="ml-2 text-stone-500">Verschieben nach:</span>
        <button
          v-for="n in neighbours"
          :key="n.event!.id"
          class="max-w-56 truncate rounded-md border border-stone-300 px-2.5 py-1 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800"
          :title="label(n.event!)"
          @click="moveTo(n.event!.id)"
        >
          {{ n.label }}: {{ label(n.event!) }}
        </button>
        <button class="rounded-md border border-stone-300 px-2.5 py-1 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="moveTo(null)">Neues Event</button>
        <button class="text-stone-500 hover:text-stone-800 dark:hover:text-stone-200" @click="selected = new Set()">Auswahl aufheben</button>
      </template>
      <span class="ml-auto text-stone-400">Nur in Sichter, an den Dateien ändert sich nichts.</span>
    </div>
    <p v-if="error" class="mx-6 mb-2 rounded-md bg-red-50 px-3 py-2 text-red-700 dark:bg-red-500/10 dark:text-red-300">{{ error }}</p>

    <div class="min-h-0 flex-1 px-6 pb-4">
      <VirtualGrid :items="tiles" :min-tile="150" :gap="12" :caption="34" :item-key="(i) => i.group_id">
        <template #default="{ item, size }">
          <button
            class="block size-full rounded-md text-left outline-offset-2"
            :class="selected.has(item.group_id) && 'outline-2 outline-sky-500'"
            @click="toggle(item.group_id)"
          >
            <GridTile :item="item" :size="size" />
          </button>
        </template>
      </VirtualGrid>
    </div>
  </div>
</template>
