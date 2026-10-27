<script setup lang="ts">
// Einzelfreigabe für "Ausschuss in den Papierkorb" (Sicherheitsmodell 3.6):
// Nichts ist vorausgewählt, jede Aufnahme muss angehakt werden.
import { computed, onMounted, ref } from "vue";
import { api, thumbUrl, type RejectGroup } from "../api";
import { fileName, formatBytes } from "../format";

const emit = defineEmits<{ close: []; planned: [runId: number] }>();
const groups = ref<RejectGroup[]>([]);
const approved = ref(new Set<number>());
const error = ref<string | null>(null);

onMounted(async () => (groups.value = await api.rejectsList()));

const size = computed(() => groups.value.filter((g) => approved.value.has(g.group_id)).reduce((s, g) => s + g.files.reduce((a, f) => a + f[1], 0), 0));

function toggle(id: number) {
  const next = new Set(approved.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  approved.value = next;
}

async function plan() {
  try {
    emit("planned", await api.planRejects(true, [...approved.value]));
  } catch (e) {
    error.value = String(e);
  }
}
</script>

<template>
  <div class="fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-8">
    <div class="flex max-h-full w-full max-w-3xl flex-col rounded-xl bg-white p-5 shadow-xl dark:bg-stone-900">
      <h2 class="text-lg font-semibold">Ausschuss in den Papierkorb</h2>
      <p class="mt-1 text-stone-500">
        Hake jede Aufnahme an, die in den Papierkorb soll. Mit ihr kommen alle zugehörigen Dateien mit (RAW, JPEG, Sidecars). Du siehst danach noch den Plan.
      </p>
      <p v-if="error" class="mt-2 rounded-md bg-red-50 px-3 py-2 text-red-700 dark:bg-red-500/10 dark:text-red-300">{{ error }}</p>
      <ul class="mt-3 min-h-0 flex-1 space-y-1.5 overflow-y-auto">
        <li v-for="g in groups" :key="g.group_id">
          <label class="flex cursor-pointer items-center gap-3 rounded-md px-2 py-1.5 hover:bg-stone-50 dark:hover:bg-stone-800/60">
            <input type="checkbox" :checked="approved.has(g.group_id)" class="size-4 accent-red-600" @change="toggle(g.group_id)" />
            <div class="size-12 shrink-0 overflow-hidden rounded bg-stone-200 dark:bg-stone-800">
              <img v-if="g.thumb" :src="thumbUrl(g.thumb)" loading="lazy" class="size-full object-cover" />
            </div>
            <div class="min-w-0 flex-1 font-mono text-[11px]">
              <div v-for="[path, bytes] in g.files" :key="path" class="flex gap-2" :title="path">
                <span class="min-w-0 flex-1 truncate">{{ fileName(path) }}</span>
                <span class="text-stone-400">{{ formatBytes(bytes) }}</span>
              </div>
            </div>
          </label>
        </li>
        <li v-if="!groups.length" class="py-6 text-center text-stone-500">Nichts als Ausschuss markiert.</li>
      </ul>
      <div class="mt-4 flex items-center gap-2">
        <button class="text-stone-500 underline-offset-2 hover:underline" @click="approved = approved.size === groups.length ? new Set() : new Set(groups.map((g) => g.group_id))">
          {{ approved.size === groups.length && groups.length ? "Keine" : "Alle" }} auswählen
        </button>
        <span class="text-stone-500 tabular-nums">{{ approved.size }} freigegeben · {{ formatBytes(size) }}</span>
        <button class="ml-auto rounded-md border border-stone-300 px-3 py-1.5 dark:border-stone-700" @click="emit('close')">Abbrechen</button>
        <button :disabled="!approved.size" class="rounded-md bg-red-600 px-3 py-1.5 text-white hover:bg-red-700 disabled:opacity-40" @click="plan">Plan erstellen</button>
      </div>
    </div>
  </div>
</template>
