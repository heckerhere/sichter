<script setup lang="ts">
// Verlauf aller Läufe (geplant, ausgeführt, abgebrochen, rückgängig gemacht).
import { ref, watch } from "vue";
import { api, type RunListItem } from "../api";
import { useRunsStore } from "../stores/runs";
import { formatInt } from "../format";
import { t } from "../i18n";

const runs = useRunsStore();
const list = ref<RunListItem[]>([]);

async function load() {
  list.value = await api.runsList();
}
watch(() => [runs.state, runs.lastFinished], load, { immediate: true, deep: true });

const statusStyle: Record<string, string> = {
  planned: "bg-stone-100 text-stone-700 dark:bg-stone-800 dark:text-stone-300",
  running: "bg-sky-100 text-sky-800 dark:bg-sky-500/15 dark:text-sky-300",
  done: "bg-emerald-50 text-emerald-800 dark:bg-emerald-500/10 dark:text-emerald-300",
  aborted: "bg-amber-100 text-amber-800 dark:bg-amber-500/15 dark:text-amber-300",
  undone: "bg-stone-100 text-stone-500 dark:bg-stone-800 dark:text-stone-400",
};
</script>

<template>
  <section class="mx-auto max-w-4xl px-8 py-8">
    <h1 class="text-xl font-semibold tracking-tight">{{ t.plan.history.title }}</h1>
    <p class="mt-2 text-stone-500 dark:text-stone-400">{{ t.plan.history.intro }}</p>
    <div v-if="!list.length" class="mt-6 rounded-lg border border-dashed border-stone-300 px-6 py-10 text-center text-stone-500 dark:border-stone-700">
      {{ t.plan.history.emptyBefore }}<RouterLink to="/umsortieren" class="underline underline-offset-2">{{ t.plan.history.emptyLink }}</RouterLink>{{ t.plan.history.emptyAfter }}
    </div>
    <ul class="mt-5 space-y-2">
      <li v-for="r in list" :key="r.id">
        <RouterLink
          :to="`/plan/${r.id}`"
          class="flex items-center gap-4 rounded-lg border border-stone-200 bg-white px-4 py-3 hover:border-stone-300 dark:border-stone-800 dark:bg-stone-900 dark:hover:border-stone-700"
        >
          <span class="w-10 text-xs text-stone-400 tabular-nums">#{{ r.id }}</span>
          <div class="min-w-0 flex-1">
            <div class="font-medium">{{ r.title }}</div>
            <div class="text-xs text-stone-500 tabular-nums">
              {{ (r.finished_at ?? r.created_at).slice(0, 16).replace("T", " ") }} UTC · {{ t.plan.history.files(formatInt(r.files)) }}
              <template v-if="r.done"> · {{ t.plan.history.done(formatInt(r.done)) }}</template>
              <template v-if="r.failed"> · <span class="text-red-600">{{ t.plan.history.failed(r.failed) }}</span></template>
              <template v-if="r.undone"> · {{ t.plan.history.undone(formatInt(r.undone)) }}</template>
            </div>
          </div>
          <span class="rounded px-2 py-0.5 text-xs" :class="statusStyle[r.status]">{{ t.plan.history.status[r.status] }}</span>
        </RouterLink>
      </li>
    </ul>
  </section>
</template>
