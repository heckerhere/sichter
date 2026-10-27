<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { thumbUrl, type EventCard } from "../api";
import { dayCount, formatInt, formatRange } from "../format";

const props = defineProps<{ event: EventCard; selected: boolean }>();
const emit = defineEmits<{
  toggleSelect: [];
  open: [];
  rename: [title: string];
  confirm: [confirmed: boolean];
}>();

const title = ref(props.event.title);
watch(
  () => props.event.title,
  (t) => (title.value = t),
);
const days = computed(() => dayCount(props.event.date_start, props.event.date_end));

function commit() {
  if (title.value.trim() !== props.event.title) emit("rename", title.value.trim());
}
</script>

<template>
  <article
    class="group flex items-center gap-4 rounded-lg border bg-white p-3 transition-colors dark:bg-stone-900"
    :class="selected ? 'border-sky-500 ring-1 ring-sky-500' : 'border-stone-200 dark:border-stone-800'"
  >
    <input type="checkbox" :checked="selected" class="size-4 shrink-0 accent-sky-600" aria-label="Für Zusammenführen auswählen" @change="emit('toggleSelect')" />

    <button class="flex shrink-0 gap-1" title="Event öffnen" @click="emit('open')">
      <div v-for="i in 4" :key="i" class="size-14 overflow-hidden rounded bg-stone-200 dark:bg-stone-800">
        <img v-if="event.thumbs[i - 1]" :src="thumbUrl(event.thumbs[i - 1])" loading="lazy" class="size-full object-cover" />
      </div>
    </button>

    <div class="min-w-0 flex-1">
      <input
        v-model="title"
        class="w-full rounded border border-transparent bg-transparent px-1 py-0.5 font-medium hover:border-stone-200 focus:border-stone-300 focus:bg-white focus:outline-none dark:hover:border-stone-700 dark:focus:border-stone-600 dark:focus:bg-stone-950"
        :placeholder="event.suggested_title || 'Ohne Titel'"
        @blur="commit"
        @keydown.enter="($event.target as HTMLInputElement).blur()"
      />
      <div class="mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-1 px-1 text-xs text-stone-500 dark:text-stone-400">
        <span class="tabular-nums">{{ formatRange(event.date_start, event.date_end) }}</span>
        <span class="tabular-nums">{{ formatInt(event.count) }} Aufnahmen<template v-if="event.video_count">, davon {{ event.video_count }} Videos</template></span>
        <span v-if="days > 1" class="rounded bg-stone-100 px-1.5 py-px dark:bg-stone-800">{{ days }} Tage</span>
        <button
          v-if="!event.title && event.suggested_title"
          class="rounded px-1.5 py-px text-stone-500 underline-offset-2 hover:underline"
          title="Vorschlag aus dem bisherigen Ordnernamen"
          @click="title = event.suggested_title; commit()"
        >
          „{{ event.suggested_title }}“ übernehmen
        </button>
        <span v-if="event.uncertain_count" class="text-amber-700 dark:text-amber-400" title="Datum stammt aus Dateiname oder Änderungsdatum">
          {{ event.uncertain_count }} mit unsicherem Datum
        </span>
        <span v-for="c in event.tz_hint_cameras" :key="c" class="text-amber-700 dark:text-amber-400" title="Videozeit passt mit der anderen Zeitzonen-Einstellung besser zu den Fotos dieser Kamera">
          Zeitzone prüfen: {{ c }}
        </span>
      </div>
    </div>

    <button
      class="shrink-0 rounded-md px-2.5 py-1 text-xs"
      :class="
        event.confirmed
          ? 'bg-emerald-50 font-medium text-emerald-800 hover:bg-emerald-100 dark:bg-emerald-500/10 dark:text-emerald-300'
          : 'border border-stone-300 text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-400 dark:hover:bg-stone-800'
      "
      :title="event.confirmed ? 'Bestätigt: wird von „Neu berechnen“ nicht verändert. Klicken zum Aufheben.' : 'Event als richtig bestätigen'"
      @click="emit('confirm', !event.confirmed)"
    >
      {{ event.confirmed ? "✓ Bestätigt" : "Bestätigen" }}
    </button>
    <button class="shrink-0 rounded-md px-2 py-1 text-xs text-stone-500 hover:bg-stone-100 dark:hover:bg-stone-800" @click="emit('open')">Öffnen</button>
  </article>
</template>
