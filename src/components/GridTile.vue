<script setup lang="ts">
import { computed, ref } from "vue";
import { thumbUrl, type GridItem } from "../api";
import { fileName, formatDate, formatDuration } from "../format";

const props = defineProps<{ item: GridItem; size: number }>();
const failed = ref(false);

const badges = computed(() => {
  const k = props.item.kinds;
  const out: string[] = [];
  if (k.includes("raw") && k.some((x) => x === "jpeg" || x === "heic")) out.push("RAW+JPEG");
  else if (k.includes("raw")) out.push("RAW");
  if (k.includes("video") && props.item.kind !== "video") out.push("Live");
  if (k.includes("sidecar")) out.push("XMP");
  return out;
});

const uncertain = computed(() => props.item.date_source && props.item.date_source !== "exif");
</script>

<template>
  <div class="flex h-full flex-col">
    <div
      class="relative overflow-hidden rounded-md bg-stone-200 dark:bg-stone-800"
      :style="{ width: `${size}px`, height: `${size}px` }"
    >
      <img
        v-if="item.thumb && !failed"
        :src="thumbUrl(item.thumb)"
        loading="lazy"
        decoding="async"
        class="size-full object-cover"
        @error="failed = true"
      />
      <div v-else class="flex size-full items-center justify-center text-[11px] font-medium tracking-wide text-stone-400 uppercase">
        {{ item.kind }}
      </div>
      <div class="absolute inset-x-1.5 top-1.5 flex flex-wrap gap-1">
        <span v-for="b in badges" :key="b" class="rounded bg-black/55 px-1 py-px text-[10px] font-medium text-white backdrop-blur-sm">{{ b }}</span>
      </div>
      <span
        v-if="item.kind === 'video' && item.duration_s"
        class="absolute right-1.5 bottom-1.5 rounded bg-black/55 px-1 py-px font-mono text-[10px] text-white tabular-nums"
      >
        {{ formatDuration(item.duration_s) }}
      </span>
    </div>
    <div class="mt-1 truncate text-[11px] text-stone-600 dark:text-stone-400" :title="item.rel_path">{{ fileName(item.rel_path) }}</div>
    <div class="truncate text-[11px] tabular-nums" :class="uncertain ? 'text-amber-700 dark:text-amber-400' : 'text-stone-400'">
      {{ formatDate(item.taken_at) }}<span v-if="uncertain" title="Kein Aufnahmedatum in den Metadaten"> · unsicher</span>
    </div>
  </div>
</template>
