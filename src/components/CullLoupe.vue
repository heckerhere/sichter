<script setup lang="ts">
// Vollbild-Sichtung mit Tastatur (PLAN.md 6.3). Bewertungen nur in der Datenbank.
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { api, mediaUrl, previewUrl, thumbUrl, type EventItem, type Flag } from "../api";
import { fileName, formatDate, formatDuration, formatInt } from "../format";

const props = defineProps<{ eventId: number | null; title: string }>();
const emit = defineEmits<{ close: [] }>();

type Filter = "all" | "unrated" | "keep" | "reject" | "stars";
const filters: { id: Filter; label: string; test: (i: EventItem) => boolean }[] = [
  { id: "all", label: "Alle", test: () => true },
  { id: "unrated", label: "Unbewertet", test: (i) => i.stars === 0 && i.flag === "none" },
  { id: "keep", label: "Behalten", test: (i) => i.flag === "keep" },
  { id: "reject", label: "Ausschuss", test: (i) => i.flag === "reject" },
  { id: "stars", label: "3+ Sterne", test: (i) => i.stars >= 3 },
];

const all = ref<EventItem[]>([]);
const filter = ref<Filter>("all");
// Gefilterte Liste als Momentaufnahme: Eine Bewertung lässt das aktuelle Bild
// nicht unter der Hand verschwinden.
const list = ref<EventItem[]>([]);
const index = ref(0);
const compare = ref(1);
const focus = ref(0);
const error = ref<string | null>(null);
const videoFailed = ref(false);
const loaded = ref(new Set<string>());
const flash = ref<string | null>(null);
const autoAdvance = ref(readSetting("cull.autoAdvance", true));

function readSetting(key: string, fallback: boolean): boolean {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : v === "1";
  } catch {
    return fallback;
  }
}
watch(autoAdvance, (v) => {
  try {
    localStorage.setItem("cull.autoAdvance", v ? "1" : "0");
  } catch {
    /* Privater Modus o. Ä.: Einstellung gilt nur für diese Sitzung. */
  }
});

function applyFilter(keepGroup?: number) {
  const test = filters.find((f) => f.id === filter.value)!.test;
  list.value = all.value.filter(test);
  const pos = keepGroup === undefined ? -1 : list.value.findIndex((i) => i.group_id === keepGroup);
  index.value = Math.max(0, Math.min(pos >= 0 ? pos : index.value, list.value.length - 1));
}

onMounted(async () => {
  try {
    all.value = await api.cullItems(props.eventId);
    applyFilter();
    prepare();
  } catch (e) {
    error.value = String(e);
  }
  window.addEventListener("keydown", onKey);
});
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
watch(filter, () => applyFilter(current.value?.group_id));

const shown = computed(() => list.value.slice(index.value, index.value + compare.value));
const current = computed(() => shown.value[Math.min(focus.value, shown.value.length - 1)] ?? null);

/** Vorschauen ab der aktuellen Position nach außen vorbereiten, Rest im Hintergrund. */
function prepare() {
  const order: string[] = [];
  const n = list.value.length;
  for (let d = 0; d < n; d++) {
    for (const i of [index.value + d, index.value - d]) {
      const h = list.value[i]?.preview_hash;
      if (h && !order.includes(h)) order.push(h);
    }
  }
  void api.previewsPrepare(order);
}

// Nachbarbilder im Browser vorladen, damit der Wechsel ohne Flackern geht.
watch([index, compare], () => {
  videoFailed.value = false;
  for (let d = -2; d <= compare.value + 2; d++) {
    const h = list.value[index.value + d]?.preview_hash;
    if (h) new Image().src = previewUrl(h);
  }
});

function go(delta: number) {
  const max = Math.max(0, list.value.length - compare.value);
  index.value = Math.max(0, Math.min(index.value + delta, max));
  focus.value = Math.min(focus.value, compare.value - 1);
}

function showFlash(text: string) {
  flash.value = text;
  setTimeout(() => flash.value === text && (flash.value = null), 700);
}

async function rate(stars: number | null, flag: Flag | null, reset = false) {
  const item = current.value;
  if (!item) return;
  const before = { stars: item.stars, flag: item.flag };
  if (reset) {
    item.stars = 0;
    item.flag = "none";
  } else {
    if (stars !== null) item.stars = stars;
    if (flag !== null) item.flag = flag;
  }
  showFlash(reset ? "Zurückgesetzt" : flag === "keep" ? "Behalten" : flag === "reject" ? "Ausschuss" : `${stars} Sterne`);
  try {
    if (reset) await api.resetRating(item.group_id);
    else await api.setRating(item.group_id, stars, flag);
  } catch (e) {
    Object.assign(item, before);
    error.value = String(e);
    return;
  }
  if (flag && autoAdvance.value && compare.value === 1) go(1);
}

function onKey(e: KeyboardEvent) {
  if ((e.target as HTMLElement)?.tagName === "INPUT" || e.metaKey || e.ctrlKey) return;
  const key = e.key.toLowerCase();
  if (key === "arrowright") go(1);
  else if (key === "arrowleft") go(-1);
  else if (key === "p") rate(null, "keep");
  else if (key === "x") rate(null, "reject");
  else if (key === "u") rate(null, null, true);
  else if (/^[0-5]$/.test(key)) rate(Number(key), null);
  else if (key === "c") compare.value = (compare.value % 4) + 1;
  else if (key === "b") showBurst();
  else if (key === "tab") focus.value = (focus.value + (e.shiftKey ? shown.value.length - 1 : 1)) % Math.max(1, shown.value.length);
  else if (key === " ") toggleVideo();
  else if (key === "escape") emit("close");
  else return;
  e.preventDefault();
}

// Funktions-Ref: Ein normales ref innerhalb von v-for wäre ein Array.
let videoEl: HTMLVideoElement | null = null;
const setVideoEl = (el: unknown) => (videoEl = el as HTMLVideoElement | null);
function toggleVideo() {
  const v = videoEl;
  if (v) v.paused ? void v.play() : v.pause();
}

/** Ganze Serie nebeneinander (bis zu 4 Bilder), beginnend beim ersten. */
function showBurst() {
  const b = current.value?.burst_id;
  if (!b) return;
  const first = list.value.findIndex((i) => i.burst_id === b);
  const size = list.value.filter((i) => i.burst_id === b).length;
  index.value = first;
  compare.value = Math.min(4, Math.max(2, size));
  focus.value = 0;
}

function burstPosition(item: EventItem) {
  if (!item.burst_id) return null;
  const members = list.value.filter((i) => i.burst_id === item.burst_id);
  return `${members.indexOf(item) + 1}/${members.length}`;
}

const openExternal = (assetId: number) => api.openInDefaultApp(assetId).catch((e) => (error.value = String(e)));

const strip = computed(() => {
  const from = Math.max(0, index.value - 30);
  return list.value.slice(from, index.value + 40).map((item, i) => ({ item, pos: from + i }));
});

const flagStyle: Record<Flag, string> = {
  keep: "bg-emerald-500 text-white",
  reject: "bg-red-500 text-white",
  none: "",
};
</script>

<template>
  <div class="fixed inset-0 z-30 flex flex-col bg-stone-950 text-stone-100 select-none">
    <!-- Kopfzeile -->
    <header class="flex items-center gap-3 px-4 pt-8 pb-2 text-xs" data-tauri-drag-region>
      <button class="rounded-md px-2 py-1 hover:bg-stone-800" @click="emit('close')">← Zurück</button>
      <span class="max-w-72 truncate font-medium text-sm">{{ title }}</span>
      <span class="text-stone-400 tabular-nums">{{ list.length ? index + 1 : 0 }} / {{ formatInt(list.length) }}</span>
      <div class="ml-4 flex gap-1">
        <button
          v-for="f in filters"
          :key="f.id"
          class="rounded px-2 py-0.5 text-stone-400 hover:bg-stone-800 hover:text-stone-100"
          :class="filter === f.id && '!bg-stone-700 !text-stone-50'"
          @click="filter = f.id"
        >
          {{ f.label }}
        </button>
      </div>
      <div class="ml-auto flex items-center gap-3">
        <label class="flex items-center gap-1.5 text-stone-400">
          <input v-model="autoAdvance" type="checkbox" class="accent-stone-200" /> Nach P/X weiter
        </label>
        <div class="flex overflow-hidden rounded-md border border-stone-700">
          <button
            v-for="n in 4"
            :key="n"
            class="px-2 py-0.5"
            :class="compare === n ? 'bg-stone-200 text-stone-900' : 'text-stone-400 hover:bg-stone-800'"
            :title="n === 1 ? 'Einzelbild' : `${n} Bilder vergleichen`"
            @click="compare = n; go(0)"
          >
            {{ n }}
          </button>
        </div>
      </div>
    </header>
    <p v-if="error" class="mx-4 rounded-md bg-red-500/15 px-3 py-1.5 text-red-300">{{ error }}</p>

    <!-- Bildfläche -->
    <main class="relative grid min-h-0 flex-1 gap-2 px-4" :style="{ gridTemplateColumns: `repeat(${shown.length || 1}, minmax(0, 1fr))` }">
      <p v-if="!list.length" class="m-auto text-stone-500">Keine Aufnahmen für diesen Filter.</p>
      <div
        v-for="(item, i) in shown"
        :key="item.group_id"
        class="relative flex min-h-0 items-center justify-center rounded-md"
        :class="compare > 1 && (i === focus ? 'ring-2 ring-sky-400' : 'ring-1 ring-stone-800')"
        @click="focus = i"
      >
        <template v-if="item.kind === 'video' && item.video_asset_id && compare === 1">
          <video
            v-if="!videoFailed"
            :ref="setVideoEl"
            :key="item.video_asset_id"
            :src="mediaUrl(item.video_asset_id)"
            :poster="item.preview_hash ? previewUrl(item.preview_hash) : undefined"
            controls
            class="max-h-full max-w-full"
            @error="videoFailed = true"
          />
          <div v-else class="flex flex-col items-center gap-3 text-center">
            <img v-if="item.preview_hash" :src="previewUrl(item.preview_hash)" class="max-h-[60vh] max-w-full rounded" />
            <p class="text-stone-400">Dieses Videoformat kann hier nicht abgespielt werden.</p>
            <button class="rounded-md bg-stone-100 px-3 py-1.5 text-stone-900 hover:bg-white" @click="openExternal(item.video_asset_id!)">
              Im Standardplayer öffnen
            </button>
          </div>
        </template>
        <template v-else-if="item.preview_hash">
          <!-- Thumbnail sofort, große Vorschau darüber, sobald geladen. -->
          <img
            v-if="item.thumb && !loaded.has(item.preview_hash)"
            :src="thumbUrl(item.thumb)"
            class="absolute max-h-full max-w-full object-contain opacity-60 blur-[1px]"
          />
          <img
            :key="item.preview_hash"
            :src="previewUrl(item.preview_hash)"
            class="relative max-h-full max-w-full object-contain"
            @load="loaded = new Set(loaded).add(item.preview_hash!)"
          />
        </template>
        <p v-else class="text-stone-500">Kein Vorschaubild verfügbar</p>

        <!-- Bewertung -->
        <div class="pointer-events-none absolute top-2 left-2 flex items-center gap-1.5">
          <span v-if="item.flag !== 'none'" class="rounded px-1.5 py-0.5 text-[11px] font-medium" :class="flagStyle[item.flag]">
            {{ item.flag === "keep" ? "Behalten" : "Ausschuss" }}
          </span>
          <span v-if="item.stars" class="rounded bg-black/60 px-1.5 py-0.5 text-[11px] tracking-wider text-amber-300">{{ "★".repeat(item.stars) }}</span>
          <span v-if="burstPosition(item)" class="rounded bg-sky-500/80 px-1.5 py-0.5 text-[11px] font-medium text-white" title="Serie: Taste B zeigt alle Bilder nebeneinander">
            Serie {{ burstPosition(item) }}
          </span>
        </div>
      </div>
      <div v-if="flash" class="pointer-events-none absolute inset-x-0 top-1/2 mx-auto w-fit -translate-y-1/2 rounded-lg bg-black/70 px-4 py-2 text-lg font-medium">
        {{ flash }}
      </div>
    </main>

    <!-- Info und Tasten -->
    <div v-if="current" class="flex items-center gap-4 px-4 py-2 text-xs text-stone-400">
      <span class="max-w-96 truncate text-stone-200" :title="current.rel_path">{{ fileName(current.rel_path) }}</span>
      <span class="tabular-nums">{{ formatDate(current.time) }}<template v-if="current.date_source && current.date_source !== 'exif'"> · Datum unsicher</template></span>
      <span v-if="current.duration_s" class="tabular-nums">{{ formatDuration(current.duration_s) }}</span>
      <span v-if="current.kinds.includes('raw') && current.kinds.some((k) => k === 'jpeg' || k === 'heic')">RAW+JPEG</span>
      <span class="ml-auto">← → blättern · P behalten · X Ausschuss · 1–5 Sterne · 0 keine Sterne · U zurücksetzen · C vergleichen · B Serie · Leertaste Video · Esc</span>
    </div>

    <!-- Filmstreifen -->
    <div class="flex gap-1 overflow-hidden px-4 pb-4">
      <button
        v-for="s in strip"
        :key="s.item.group_id"
        class="relative size-14 shrink-0 overflow-hidden rounded bg-stone-800"
        :class="[
          s.pos >= index && s.pos < index + compare ? 'ring-2 ring-stone-100' : 'opacity-60 hover:opacity-100',
          s.item.burst_id && 'border-t-2 border-sky-400',
        ]"
        @click="index = s.pos; go(0)"
      >
        <img v-if="s.item.thumb" :src="thumbUrl(s.item.thumb)" loading="lazy" class="size-full object-cover" />
        <span v-if="s.item.flag !== 'none'" class="absolute inset-x-0 bottom-0 h-1" :class="s.item.flag === 'keep' ? 'bg-emerald-500' : 'bg-red-500'" />
        <span v-if="s.item.stars" class="absolute top-0.5 right-0.5 rounded bg-black/60 px-0.5 text-[9px] text-amber-300">{{ s.item.stars }}★</span>
      </button>
    </div>
  </div>
</template>
