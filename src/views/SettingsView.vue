<script setup lang="ts">
import { ask } from "@tauri-apps/plugin-dialog";
import SystemStatus from "../components/SystemStatus.vue";
import { useAppStore } from "../stores/app";

const app = useAppStore();

async function toggleDryRun() {
  if (app.dryRun) {
    const ok = await ask("Danach kannst du Pläne wirklich ausführen. Jede Ausführung fragt weiterhin mit Dateianzahl nach.", {
      title: "Dry-Run-Modus ausschalten",
      okLabel: "Ausschalten",
      cancelLabel: "Abbrechen",
    });
    if (!ok) return;
  }
  await app.setDryRun(!app.dryRun);
}
</script>

<template>
  <section class="mx-auto max-w-3xl px-8 py-10">
    <h1 class="text-xl font-semibold tracking-tight">Einstellungen</h1>
    <div class="mt-6 flex items-center gap-4 rounded-lg border border-stone-200 bg-white p-4 dark:border-stone-800 dark:bg-stone-900">
      <div class="flex-1">
        <h2 class="font-medium">Dry-Run-Modus</h2>
        <p class="mt-0.5 text-stone-500 dark:text-stone-400">
          Solange er an ist, lassen sich Pläne nur als Probelauf durchspielen. Es wird keine Datei verändert. Standard ist an.
        </p>
      </div>
      <button
        class="rounded-md px-3 py-1.5"
        :class="app.dryRun ? 'bg-amber-100 font-medium text-amber-800 dark:bg-amber-500/15 dark:text-amber-300' : 'border border-stone-300 dark:border-stone-700'"
        @click="toggleDryRun"
      >
        {{ app.dryRun ? "An" : "Aus" }}
      </button>
    </div>
    <h2 class="mt-8 mb-3 text-xs font-medium tracking-wide text-stone-500 uppercase dark:text-stone-400">Systemstatus</h2>
    <SystemStatus />
  </section>
</template>
