<script setup lang="ts">
// Umsortieren (PLAN.md 6.5): Vorlage festlegen, Plan erzeugen. Ausgeführt wird
// erst in der Plan-Ansicht nach Vorschau und Bestätigung.
import { computed, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { api, type ReorganizeParams } from "../api";
import { useLibraryStore } from "../stores/library";

const lib = useLibraryStore();
const router = useRouter();

const PLACEHOLDERS = ["{Jahr}", "{Monat}", "{Datum}", "{Anlass}", "{Ort}", "{Kamera}", "{Original-Dateiname}"];
const DEFAULT = "{Jahr}/{Datum} {Anlass}/{Original-Dateiname}";

const usable = computed(() => lib.roots.filter((r) => !r.locked && r.reachable));
const params = ref<ReorganizeParams>({
  source_root_ids: [],
  target_root_id: 0,
  template: DEFAULT,
  video_template: null,
  mode: "move",
  date_range: false,
  only_confirmed: false,
});
const separateVideos = ref(false);
const videoTemplate = ref("{Jahr}/{Datum} {Anlass}/Videos/{Original-Dateiname}");
const preview = ref<{ error: string | null; samples: [string, string][] } | null>(null);
const error = ref<string | null>(null);
const busy = ref(false);
const templateInput = ref<HTMLInputElement | null>(null);

watch(
  usable,
  (roots) => {
    if (!params.value.source_root_ids.length) params.value.source_root_ids = roots.map((r) => r.id);
    if (!roots.some((r) => r.id === params.value.target_root_id)) params.value.target_root_id = roots[0]?.id ?? 0;
  },
  { immediate: true },
);

const effective = computed<ReorganizeParams>(() => ({
  ...params.value,
  video_template: separateVideos.value ? videoTemplate.value : null,
}));

let timer: ReturnType<typeof setTimeout> | undefined;
watch(
  effective,
  (p) => {
    clearTimeout(timer);
    if (!p.target_root_id || !p.source_root_ids.length) return;
    timer = setTimeout(async () => (preview.value = await api.planTemplatePreview(p)), 250);
  },
  { deep: true, immediate: true },
);

function insert(token: string) {
  const el = templateInput.value;
  const t = params.value.template;
  const pos = el?.selectionStart ?? t.length;
  params.value.template = t.slice(0, pos) + token + t.slice(el?.selectionEnd ?? pos);
}

function toggleSource(id: number) {
  const ids = new Set(params.value.source_root_ids);
  if (ids.has(id)) ids.delete(id);
  else ids.add(id);
  params.value.source_root_ids = [...ids];
}

const rootName = (path: string) => path.slice(path.lastIndexOf("/") + 1);

async function createPlan() {
  busy.value = true;
  error.value = null;
  try {
    const id = await api.planReorganize(effective.value);
    await router.push(`/plan/${id}`);
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <section class="mx-auto max-w-4xl px-8 py-8">
    <h1 class="text-xl font-semibold tracking-tight">Umsortieren</h1>
    <p class="mt-2 max-w-2xl text-stone-500 dark:text-stone-400">
      Lege fest, wie deine Fotos geordnet werden sollen. Sichter erstellt daraus einen Plan, den du vollständig siehst, bevor irgendetwas passiert. Nichts wird
      überschrieben, und jeder Lauf lässt sich rückgängig machen.
    </p>

    <div v-if="!usable.length" class="mt-6 rounded-lg border border-dashed border-stone-300 px-6 py-10 text-center text-stone-500 dark:border-stone-700">
      Kein erreichbarer, nicht gesperrter Ordner in der Bibliothek.
    </div>

    <div v-else class="mt-6 space-y-5">
      <div class="grid gap-5 rounded-lg border border-stone-200 bg-white p-5 sm:grid-cols-2 dark:border-stone-800 dark:bg-stone-900">
        <div>
          <h2 class="text-xs font-medium tracking-wide text-stone-500 uppercase">Quelle</h2>
          <label v-for="r in lib.roots" :key="r.id" class="mt-2 flex items-center gap-2" :class="(r.locked || !r.reachable) && 'opacity-50'" :title="r.path">
            <input
              type="checkbox"
              :disabled="r.locked || !r.reachable"
              :checked="params.source_root_ids.includes(r.id)"
              class="accent-stone-800 dark:accent-stone-200"
              @change="toggleSource(r.id)"
            />
            <span class="truncate">{{ rootName(r.path) }}</span>
            <span v-if="r.locked" class="text-xs text-stone-400">gesperrt</span>
            <span v-else-if="!r.reachable" class="text-xs text-stone-400">nicht erreichbar</span>
          </label>
        </div>
        <div>
          <h2 class="text-xs font-medium tracking-wide text-stone-500 uppercase">Ziel</h2>
          <select v-model="params.target_root_id" class="mt-2 w-full rounded-md border border-stone-300 bg-transparent px-2 py-1 dark:border-stone-700">
            <option v-for="r in usable" :key="r.id" :value="r.id">{{ rootName(r.path) }}</option>
          </select>
          <p class="mt-1 text-[11px] text-stone-400">Das Ziel muss ein Ordner der Bibliothek sein und darf auch die Quelle selbst sein.</p>
          <div class="mt-3 flex overflow-hidden rounded-md border border-stone-300 text-xs dark:border-stone-700">
            <button
              v-for="m in ['move', 'copy'] as const"
              :key="m"
              class="flex-1 px-3 py-1.5"
              :class="params.mode === m ? 'bg-stone-900 text-white dark:bg-stone-100 dark:text-stone-900' : 'hover:bg-stone-100 dark:hover:bg-stone-800'"
              @click="params.mode = m"
            >
              {{ m === "move" ? "Verschieben (Standard)" : "Kopieren, Quelle bleibt" }}
            </button>
          </div>
        </div>
      </div>

      <div class="rounded-lg border border-stone-200 bg-white p-5 dark:border-stone-800 dark:bg-stone-900">
        <h2 class="text-xs font-medium tracking-wide text-stone-500 uppercase">Vorlage</h2>
        <input
          ref="templateInput"
          v-model="params.template"
          spellcheck="false"
          class="mt-2 w-full rounded-md border border-stone-300 bg-transparent px-3 py-2 font-mono text-sm dark:border-stone-700"
        />
        <div class="mt-2 flex flex-wrap gap-1.5">
          <button
            v-for="p in PLACEHOLDERS"
            :key="p"
            class="rounded bg-stone-100 px-2 py-0.5 font-mono text-xs text-stone-700 hover:bg-stone-200 dark:bg-stone-800 dark:text-stone-300 dark:hover:bg-stone-700"
            @click="insert(p)"
          >
            {{ p }}
          </button>
          <button class="ml-auto text-xs text-stone-500 underline-offset-2 hover:underline" @click="params.template = DEFAULT">Standard</button>
        </div>
        <div class="mt-4 flex flex-wrap gap-x-6 gap-y-2">
          <label class="flex items-center gap-2"><input v-model="params.date_range" type="checkbox" class="accent-stone-800 dark:accent-stone-200" /> Mehrtägige Events als „2024-07-12 bis 2024-07-20“</label>
          <label class="flex items-center gap-2"><input v-model="separateVideos" type="checkbox" class="accent-stone-800 dark:accent-stone-200" /> Eigene Vorlage für Videos</label>
          <label class="flex items-center gap-2" title="Aufnahmen in unbestätigten Events bleiben liegen">
            <input v-model="params.only_confirmed" type="checkbox" class="accent-stone-800 dark:accent-stone-200" /> Nur bestätigte Events
          </label>
        </div>
        <input
          v-if="separateVideos"
          v-model="videoTemplate"
          spellcheck="false"
          class="mt-3 w-full rounded-md border border-stone-300 bg-transparent px-3 py-2 font-mono text-sm dark:border-stone-700"
        />
        <p class="mt-3 text-[11px] text-stone-400">
          Aufnahmen ohne Event landen in {Jahr}/_Unsortiert. RAW+JPEG, Sidecars und Live Photos bleiben immer zusammen. Unbekannte Dateitypen werden nicht bewegt.
        </p>

        <div class="mt-4 rounded-md bg-stone-50 p-3 dark:bg-stone-950">
          <h3 class="text-xs font-medium text-stone-500">Vorschau</h3>
          <p v-if="preview?.error" class="mt-1 text-red-600 dark:text-red-400">{{ preview.error }}</p>
          <p v-else-if="preview && !preview.samples.length" class="mt-1 text-stone-500">Alles liegt bereits so wie in der Vorlage.</p>
          <ul v-else-if="preview" class="mt-1 space-y-1 font-mono text-[11px]">
            <li v-for="([from, to], i) in preview.samples" :key="i" class="grid grid-cols-[1fr_auto_1fr] gap-2">
              <span class="truncate text-stone-400" :title="from">{{ from.slice(from.lastIndexOf("/", from.lastIndexOf("/") - 1) + 1) }}</span>
              <span class="text-stone-400">→</span>
              <span class="truncate" :title="to">{{ to }}</span>
            </li>
          </ul>
        </div>
      </div>

      <p v-if="error" class="rounded-md bg-red-50 px-3 py-2 text-red-700 dark:bg-red-500/10 dark:text-red-300">{{ error }}</p>
      <div class="flex justify-end">
        <button
          :disabled="busy || !!preview?.error || !params.source_root_ids.length"
          class="rounded-md bg-stone-900 px-4 py-2 text-white hover:bg-stone-700 disabled:opacity-40 dark:bg-stone-100 dark:text-stone-900"
          @click="createPlan"
        >
          {{ busy ? "Plan wird erstellt …" : "Plan erstellen und ansehen" }}
        </button>
      </div>
    </div>
  </section>
</template>
