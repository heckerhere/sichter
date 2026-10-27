<script setup lang="ts">
import { useAppStore } from "../stores/app";
import type { ToolSource } from "../api";

const app = useAppStore();

const sourceLabel: Record<ToolSource, string> = {
  bundled: "gebündelt",
  path: "System-PATH",
  env: "Umgebungsvariable",
};
</script>

<template>
  <section class="mx-auto max-w-3xl px-8 py-10">
    <h1 class="text-xl font-semibold tracking-tight">Bibliothek</h1>

    <div class="mt-6 rounded-lg border border-dashed border-stone-300 px-6 py-10 text-center dark:border-stone-700">
      <p class="font-medium">Noch keine Ordner hinzugefügt</p>
      <p class="mx-auto mt-1 max-w-sm text-stone-500 dark:text-stone-400">
        Sichter liest deine Ordner nur. Änderungen passieren ausschließlich über einen Plan, den du vorher prüfst und bestätigst.
      </p>
      <button disabled class="mt-4 cursor-not-allowed rounded-md bg-stone-900 px-3 py-1.5 text-white opacity-40 dark:bg-stone-100 dark:text-stone-900">
        Ordner hinzufügen
      </button>
    </div>

    <h2 class="mt-10 mb-3 text-xs font-medium tracking-wide text-stone-500 uppercase dark:text-stone-400">Systemstatus</h2>

    <p v-if="app.error" class="rounded-md bg-red-50 px-3 py-2 text-red-700 dark:bg-red-500/10 dark:text-red-300">
      {{ app.error }}
    </p>
    <p v-else-if="!app.info" class="text-stone-500">Werkzeuge werden geprüft …</p>

    <div v-else class="overflow-hidden rounded-lg border border-stone-200 bg-white dark:border-stone-800 dark:bg-stone-900">
      <table class="w-full text-left">
        <tbody class="divide-y divide-stone-100 dark:divide-stone-800">
          <tr v-for="tool in app.info.tools" :key="tool.name" class="align-top">
            <td class="w-28 px-4 py-3 font-medium">{{ tool.name }}</td>
            <td class="px-4 py-3">
              <div class="flex flex-wrap items-center gap-2">
                <span v-if="tool.version" class="font-mono tabular-nums">{{ tool.version }}</span>
                <span v-else class="font-medium text-red-600 dark:text-red-400">{{ tool.error }}</span>
                <span v-if="tool.source" class="rounded bg-stone-100 px-1.5 py-px text-[11px] text-stone-600 dark:bg-stone-800 dark:text-stone-300">
                  {{ sourceLabel[tool.source] }}
                </span>
                <span
                  v-if="tool.gpl"
                  class="rounded bg-amber-100 px-1.5 py-px text-[11px] text-amber-800 dark:bg-amber-500/15 dark:text-amber-300"
                  title="Für Releases wird ein LGPL-Build gebündelt."
                >
                  GPL-Build, nur Entwicklung
                </span>
              </div>
              <div v-if="tool.path" class="mt-1 truncate font-mono text-[11px] text-stone-400 select-text" :title="tool.path">{{ tool.path }}</div>
              <div v-if="tool.version && tool.error" class="mt-1 text-red-600">{{ tool.error }}</div>
            </td>
          </tr>
          <tr>
            <td class="px-4 py-3 font-medium">Datenbank</td>
            <td class="px-4 py-3">
              <span class="font-mono">Schema v{{ app.info.db_schema_version }}</span>
              <div class="mt-1 truncate font-mono text-[11px] text-stone-400 select-text" :title="app.info.data_dir">{{ app.info.data_dir }}</div>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
    <p v-if="app.info" class="mt-3 text-xs text-stone-400">Sichter {{ app.info.app_version }}</p>
  </section>
</template>
