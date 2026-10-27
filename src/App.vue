<script setup lang="ts">
import { onMounted } from "vue";
import { sections } from "./router";
import NavIcon from "./components/NavIcon.vue";
import ScanStatus from "./components/ScanStatus.vue";
import { useAppStore } from "./stores/app";
import { useLibraryStore } from "./stores/library";
import { formatInt } from "./format";

const app = useAppStore();
const lib = useLibraryStore();
onMounted(() => {
  app.load();
  lib.init();
});
</script>

<template>
  <div class="grid h-full grid-cols-[208px_1fr] grid-rows-[1fr_auto]">
    <nav class="row-span-1 flex flex-col gap-0.5 border-r border-stone-200 bg-stone-100/70 px-2.5 pt-10 pb-3 dark:border-stone-800 dark:bg-stone-900/60" data-tauri-drag-region>
      <div class="mb-4 px-2.5 text-[15px] font-semibold tracking-tight">Sichter</div>
      <RouterLink
        v-for="s in sections"
        :key="s.path"
        :to="s.path"
        class="flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-stone-600 transition-colors hover:bg-stone-200/70 hover:text-stone-900 dark:text-stone-400 dark:hover:bg-stone-800 dark:hover:text-stone-100"
        active-class="!bg-stone-200 !text-stone-900 dark:!bg-stone-800 dark:!text-stone-50"
      >
        <NavIcon :name="s.icon" class="size-4 shrink-0" />
        <span>{{ s.name }}</span>
      </RouterLink>
    </nav>

    <main class="min-h-0 overflow-y-auto">
      <RouterView />
    </main>

    <footer class="col-span-2 flex items-center gap-4 border-t border-stone-200 bg-stone-100/70 px-4 py-1.5 text-xs text-stone-500 dark:border-stone-800 dark:bg-stone-900/60 dark:text-stone-400">
      <span class="tabular-nums">{{ formatInt(lib.stats?.files ?? 0) }} Dateien</span>
      <ScanStatus />
      <template v-if="!lib.scanning">
        <span>Keine offenen Pläne</span>
        <span>Noch kein Lauf</span>
      </template>
      <span
        v-if="app.dryRun"
        class="ml-auto shrink-0 rounded bg-amber-100 px-2 py-0.5 font-medium text-amber-800 dark:bg-amber-500/15 dark:text-amber-300"
        title="Pläne werden nur simuliert, es wird keine Datei verändert."
      >
        Dry-Run aktiv
      </span>
    </footer>
  </div>
</template>
