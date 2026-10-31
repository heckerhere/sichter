<script setup lang="ts">
// Kompakte Scan-Anzeige für die Statusleiste, auf jeder Seite sichtbar.
import { computed } from "vue";
import { useLibraryStore } from "../stores/library";
import { formatInt } from "../format";
import { t } from "../i18n";
import { overallPercent, phaseLabel, phaseStep } from "../scanPhases";

const lib = useLibraryStore();
const percent = computed(() => overallPercent(lib.progress));
const step = computed(() => phaseStep(lib.progress?.phase));
</script>

<template>
  <div v-if="lib.scanning" class="flex min-w-0 items-center gap-2.5">
    <span class="relative flex size-2 shrink-0">
      <span class="absolute inline-flex size-full animate-ping rounded-full bg-sky-500 opacity-60" />
      <span class="relative inline-flex size-2 rounded-full bg-sky-500" />
    </span>
    <span class="shrink-0 font-medium text-stone-700 dark:text-stone-200">{{ t.app.scan.percent(percent) }}</span>
    <div class="h-1.5 w-40 shrink-0 overflow-hidden rounded-full bg-stone-200 dark:bg-stone-700" role="progressbar" :aria-valuenow="percent" aria-valuemin="0" aria-valuemax="100">
      <div class="h-full rounded-full bg-sky-500 transition-[width] duration-300" :style="{ width: `${Math.max(2, percent)}%` }" />
    </div>
    <span class="min-w-0 truncate">
      {{ step.step }}/{{ step.of }} {{ phaseLabel(lib.progress?.phase) }}<template v-if="lib.progress && lib.progress.done">
        · {{ formatInt(lib.progress.done) }}<template v-if="lib.progress.total"> / {{ formatInt(lib.progress.total) }}</template></template>
    </span>
    <button class="shrink-0 rounded px-1.5 py-px hover:bg-stone-200 hover:text-stone-900 dark:hover:bg-stone-700 dark:hover:text-stone-100" @click="lib.cancelScan">
      {{ t.app.scan.cancel }}
    </button>
  </div>
</template>
