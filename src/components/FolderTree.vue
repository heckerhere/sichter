<script setup lang="ts">
// Ordnerbaum mit Größen. Rein lesend: Aktionen gibt es nur über die anderen Bereiche.
import { computed, ref, watch } from "vue";
import type { FolderRow } from "../api";
import { formatBytes, formatInt } from "../format";
import { locale, t } from "../i18n";
import KindBar from "./KindBar.vue";

const props = defineProps<{ folders: FolderRow[]; rootLabel: string }>();
const emit = defineEmits<{ select: [folder: FolderRow] }>();

type Sort = "size" | "name" | "count";
const sort = ref<Sort>("size");
const expanded = ref(new Set<number>());

const byId = computed(() => new Map(props.folders.map((f) => [f.id, f])));
const children = computed(() => {
  const map = new Map<number | null, FolderRow[]>();
  for (const f of props.folders) {
    const list = map.get(f.parent_id) ?? [];
    list.push(f);
    map.set(f.parent_id, list);
  }
  const cmp: Record<Sort, (a: FolderRow, b: FolderRow) => number> = {
    size: (a, b) => b.total_size - a.total_size,
    count: (a, b) => b.file_count - a.file_count,
    name: (a, b) => name(a).localeCompare(name(b), locale.value, { numeric: true }),
  };
  for (const list of map.values()) list.sort(cmp[sort.value]);
  return map;
});

const root = computed(() => children.value.get(null)?.[0]);

// Root und erste Ebene sind beim Laden aufgeklappt.
watch(
  root,
  (r) => {
    if (r && !expanded.value.size) expanded.value = new Set([r.id]);
  },
  { immediate: true },
);

function name(f: FolderRow) {
  if (!f.rel_path) return props.rootLabel;
  return f.rel_path.slice(f.rel_path.lastIndexOf("/") + 1);
}

const rows = computed(() => {
  const out: { folder: FolderRow; depth: number; share: number | null; hasChildren: boolean }[] = [];
  const visit = (f: FolderRow, depth: number) => {
    const parent = f.parent_id !== null ? byId.value.get(f.parent_id) : undefined;
    const kids = children.value.get(f.id) ?? [];
    out.push({ folder: f, depth, share: parent && parent.total_size ? f.total_size / parent.total_size : null, hasChildren: kids.length > 0 });
    if (expanded.value.has(f.id)) kids.forEach((k) => visit(k, depth + 1));
  };
  if (root.value) visit(root.value, 0);
  return out;
});

function toggle(id: number) {
  const next = new Set(expanded.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  expanded.value = next;
}

const sortButtons: Sort[] = ["size", "count", "name"];
</script>

<template>
  <div class="overflow-hidden rounded-lg border border-stone-200 bg-white dark:border-stone-800 dark:bg-stone-900">
    <div class="flex items-center gap-1 border-b border-stone-100 px-3 py-2 text-xs dark:border-stone-800">
      <span class="mr-1 text-stone-500">{{ t.storage.tree.sortBy }}</span>
      <button
        v-for="b in sortButtons"
        :key="b"
        class="rounded px-2 py-0.5 text-stone-600 hover:bg-stone-100 dark:text-stone-400 dark:hover:bg-stone-800"
        :class="sort === b && '!bg-stone-200 !text-stone-900 dark:!bg-stone-800 dark:!text-stone-50'"
        @click="sort = b"
      >
        {{ t.storage.tree.sort[b] }}
      </button>
    </div>
    <table class="w-full table-fixed text-left">
      <thead class="text-[11px] text-stone-500 dark:text-stone-400">
        <tr class="border-b border-stone-100 dark:border-stone-800">
          <th class="px-3 py-1.5 font-medium">{{ t.storage.tree.folder }}</th>
          <th class="w-24 px-3 py-1.5 text-right font-medium">{{ t.storage.tree.total }}</th>
          <th class="w-24 px-3 py-1.5 text-right font-medium">{{ t.storage.tree.own }}</th>
          <th class="w-20 px-3 py-1.5 text-right font-medium">{{ t.storage.tree.files }}</th>
          <th class="w-28 px-3 py-1.5 font-medium">{{ t.storage.tree.share }}</th>
          <th class="w-44 px-3 py-1.5 font-medium">{{ t.storage.tree.byType }}</th>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="r in rows"
          :key="r.folder.id"
          class="cursor-default border-b border-stone-50 last:border-0 hover:bg-stone-50 dark:border-stone-800/60 dark:hover:bg-stone-800/50"
          @click="emit('select', r.folder)"
        >
          <td class="truncate px-3 py-1.5" :title="r.folder.rel_path || rootLabel">
            <span :style="{ paddingLeft: `${r.depth * 16}px` }" class="inline-flex items-center gap-1">
              <button
                v-if="r.hasChildren"
                class="flex size-4 items-center justify-center rounded text-stone-400 hover:bg-stone-200 hover:text-stone-700 dark:hover:bg-stone-700"
                :aria-label="expanded.has(r.folder.id) ? t.storage.tree.collapse : t.storage.tree.expand"
                @click.stop="toggle(r.folder.id)"
              >
                <svg viewBox="0 0 16 16" class="size-3 transition-transform" :class="expanded.has(r.folder.id) && 'rotate-90'" fill="currentColor"><path d="M6 4l4 4-4 4z" /></svg>
              </button>
              <span v-else class="size-4" />
              <span :class="r.depth === 0 && 'font-medium'">{{ name(r.folder) }}</span>
            </span>
          </td>
          <td class="px-3 py-1.5 text-right tabular-nums">{{ formatBytes(r.folder.total_size) }}</td>
          <td class="px-3 py-1.5 text-right text-stone-500 tabular-nums">{{ r.folder.own_size ? formatBytes(r.folder.own_size) : "–" }}</td>
          <td class="px-3 py-1.5 text-right text-stone-500 tabular-nums">{{ formatInt(r.folder.file_count) }}</td>
          <td class="px-3 py-1.5">
            <div v-if="r.share !== null" class="flex items-center gap-2">
              <div class="h-1.5 flex-1 overflow-hidden rounded-full" style="background: var(--viz-track)">
                <div class="h-full rounded-full bg-stone-500 dark:bg-stone-400" :style="{ width: `${Math.max(1, r.share * 100)}%` }" />
              </div>
              <span class="w-9 text-right text-[11px] text-stone-500 tabular-nums">{{ t.storage.percent(Math.round(r.share * 100)) }}</span>
            </div>
          </td>
          <td class="px-3 py-1.5"><KindBar :kinds="r.folder.kinds" /></td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
