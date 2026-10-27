<script setup lang="ts" generic="T">
// Virtualisiertes Grid: rendert nur die sichtbaren Zeilen plus Puffer,
// damit auch 100.000 Kacheln flüssig scrollen.
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

const props = defineProps<{
  items: T[];
  minTile: number;
  gap: number;
  /** Höhe unter der quadratischen Kachel (Bildunterschrift). */
  caption: number;
  itemKey: (item: T) => string | number;
}>();

const el = ref<HTMLElement | null>(null);
const width = ref(0);
const height = ref(0);
const scrollTop = ref(0);
const OVERSCAN = 3;

const columns = computed(() => Math.max(1, Math.floor((width.value + props.gap) / (props.minTile + props.gap))));
const tile = computed(() => (width.value - props.gap * (columns.value - 1)) / columns.value);
const rowHeight = computed(() => tile.value + props.caption + props.gap);
const rows = computed(() => Math.ceil(props.items.length / columns.value));
const firstRow = computed(() => Math.max(0, Math.floor(scrollTop.value / rowHeight.value) - OVERSCAN));
const lastRow = computed(() => Math.min(rows.value, Math.ceil((scrollTop.value + height.value) / rowHeight.value) + OVERSCAN));

const visible = computed(() => {
  const out: { item: T; top: number; left: number }[] = [];
  for (let row = firstRow.value; row < lastRow.value; row++) {
    for (let col = 0; col < columns.value; col++) {
      const item = props.items[row * columns.value + col];
      if (item === undefined) break;
      out.push({ item, top: row * rowHeight.value, left: col * (tile.value + props.gap) });
    }
  }
  return out;
});

let observer: ResizeObserver | null = null;
onMounted(() => {
  observer = new ResizeObserver(([entry]) => {
    width.value = entry.contentRect.width;
    height.value = entry.contentRect.height;
  });
  if (el.value) observer.observe(el.value);
});
onBeforeUnmount(() => observer?.disconnect());
</script>

<template>
  <div ref="el" class="relative h-full overflow-y-auto" @scroll="scrollTop = ($event.target as HTMLElement).scrollTop">
    <div :style="{ height: `${rows * rowHeight}px` }" class="relative">
      <div
        v-for="v in visible"
        :key="itemKey(v.item)"
        class="absolute"
        :style="{ top: `${v.top}px`, left: `${v.left}px`, width: `${tile}px`, height: `${tile + caption}px` }"
      >
        <slot :item="v.item" :size="tile" />
      </div>
    </div>
  </div>
</template>
