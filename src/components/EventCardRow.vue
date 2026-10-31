<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { thumbUrl, type EventCard } from "../api";
import { t } from "../i18n";
import { countryName, dayCount, formatInt, formatRange } from "../format";

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
  (v) => (title.value = v),
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
    <input type="checkbox" :checked="selected" class="size-4 shrink-0 accent-sky-600" :aria-label="t.events.card.selectForMerge" @change="emit('toggleSelect')" />

    <button class="flex shrink-0 gap-1" :title="t.events.card.openEvent" @click="emit('open')">
      <div v-for="i in 4" :key="i" class="size-14 overflow-hidden rounded bg-stone-200 dark:bg-stone-800">
        <img v-if="event.thumbs[i - 1]" :src="thumbUrl(event.thumbs[i - 1])" loading="lazy" class="size-full object-cover" />
      </div>
    </button>

    <div class="min-w-0 flex-1">
      <input
        v-model="title"
        class="w-full rounded border border-transparent bg-transparent px-1 py-0.5 font-medium hover:border-stone-200 focus:border-stone-300 focus:bg-white focus:outline-none dark:hover:border-stone-700 dark:focus:border-stone-600 dark:focus:bg-stone-950"
        :placeholder="event.suggested_title || t.events.card.untitled"
        @blur="commit"
        @keydown.enter="($event.target as HTMLInputElement).blur()"
      />
      <div class="mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-1 px-1 text-xs text-stone-500 dark:text-stone-400">
        <span class="tabular-nums">{{ formatRange(event.date_start, event.date_end) }}</span>
        <span class="tabular-nums">{{ t.events.card.photos(formatInt(event.count)) }}<template v-if="event.video_count">{{ t.events.card.videosOf(event.video_count) }}</template></span>
        <span v-if="days > 1" class="rounded bg-stone-100 px-1.5 py-px dark:bg-stone-800">{{ t.events.card.days(days) }}</span>
        <span v-if="event.place" :title="t.events.card.placeHint">{{ event.place.name }}, {{ countryName(event.place.country) }}</span>
        <button
          v-if="!event.title && event.suggested_title"
          class="rounded px-1.5 py-px text-stone-500 underline-offset-2 hover:underline"
          :title="t.events.card.suggestionHint"
          @click="title = event.suggested_title; commit()"
        >
          {{ t.events.card.useSuggestion(event.suggested_title) }}
        </button>
        <span v-if="event.uncertain_count" class="text-amber-700 dark:text-amber-400" :title="t.events.card.uncertainHint">
          {{ t.events.card.uncertain(event.uncertain_count) }}
        </span>
        <span v-for="c in event.tz_hint_cameras" :key="c" class="text-amber-700 dark:text-amber-400" :title="t.events.card.tzHint">
          {{ t.events.card.tzCheck(c) }}
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
      :title="event.confirmed ? t.events.card.confirmedHint : t.events.card.confirmHint"
      @click="emit('confirm', !event.confirmed)"
    >
      {{ event.confirmed ? t.events.card.confirmed : t.events.card.confirm }}
    </button>
    <button class="shrink-0 rounded-md px-2 py-1 text-xs text-stone-500 hover:bg-stone-100 dark:hover:bg-stone-800" @click="emit('open')">{{ t.events.card.open }}</button>
  </article>
</template>
