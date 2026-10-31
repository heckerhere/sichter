<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { ask } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import SystemStatus from "../components/SystemStatus.vue";
import { useAppStore } from "../stores/app";
import { formatBytes } from "../format";
import { t, type LanguagePref } from "../i18n";

const app = useAppStore();
const router = useRouter();
const cache = ref<{ data_dir: string; thumbs: number; previews: number; logs: number } | null>(null);
const error = ref<string | null>(null);

const languages: LanguagePref[] = ["auto", "de", "en"];
async function setLanguage(pref: LanguagePref) {
  try {
    await app.applyLanguage(pref);
  } catch (e) {
    error.value = String(e);
  }
}

const xmp = ref(false);
const loadCache = async () => (cache.value = await api.cacheInfo());
onMounted(async () => {
  await loadCache();
  xmp.value = await api.xmpExportEnabled();
});

async function toggleXmp() {
  await api.setXmpExportEnabled(!xmp.value);
  xmp.value = !xmp.value;
}

async function toggleDryRun() {
  if (app.dryRun) {
    const ok = await ask(t.settings.dryRun.confirmMessage, {
      title: t.settings.dryRun.confirmTitle,
      okLabel: t.settings.dryRun.confirmOk,
      cancelLabel: t.common.cancel,
    });
    if (!ok) return;
  }
  await app.setDryRun(!app.dryRun);
}

async function clear(which: "thumbs" | "previews") {
  const ok = await ask(
    which === "thumbs"
      ? t.settings.cache.confirmThumbs
      : t.settings.cache.confirmPreviews,
    { title: t.settings.cache.confirmTitle, okLabel: t.settings.cache.confirmOk, cancelLabel: t.common.cancel },
  );
  if (!ok) return;
  await api.cacheClear(which);
  await loadCache();
}

async function emptyDirs() {
  try {
    await router.push(`/plan/${await api.planEmptyDirs()}`);
  } catch (e) {
    error.value = String(e);
  }
}

const openDataDir = () => api.openDataDir().catch((e) => (error.value = String(e)));
</script>

<template>
  <section class="mx-auto max-w-3xl px-8 py-10">
    <h1 class="text-xl font-semibold tracking-tight">{{ t.settings.title }}</h1>
    <p v-if="error" class="mt-3 rounded-md bg-red-50 px-3 py-2 text-red-700 dark:bg-red-500/10 dark:text-red-300">{{ error }}</p>

    <div class="mt-6 flex items-center gap-4 rounded-lg border border-stone-200 bg-white p-4 dark:border-stone-800 dark:bg-stone-900">
      <div class="flex-1">
        <h2 class="font-medium">{{ t.settings.language.title }}</h2>
        <p class="mt-0.5 text-stone-500 dark:text-stone-400">{{ t.settings.language.hint }}</p>
      </div>
      <div class="flex shrink-0 gap-1 rounded-md border border-stone-300 p-0.5 dark:border-stone-700">
        <button
          v-for="l in languages"
          :key="l"
          class="rounded px-2.5 py-1 text-stone-600 hover:bg-stone-100 dark:text-stone-400 dark:hover:bg-stone-800"
          :class="app.language === l && '!bg-stone-900 !text-white dark:!bg-stone-100 dark:!text-stone-900'"
          :lang="l === 'auto' ? undefined : l"
          @click="setLanguage(l)"
        >
          {{ t.settings.language[l] }}
        </button>
      </div>
    </div>

    <div class="mt-3 flex items-center gap-4 rounded-lg border border-stone-200 bg-white p-4 dark:border-stone-800 dark:bg-stone-900">
      <div class="flex-1">
        <h2 class="font-medium">{{ t.settings.dryRun.title }}</h2>
        <p class="mt-0.5 text-stone-500 dark:text-stone-400">
          {{ t.settings.dryRun.hint }}
        </p>
      </div>
      <button
        class="rounded-md px-3 py-1.5"
        :class="app.dryRun ? 'bg-amber-100 font-medium text-amber-800 dark:bg-amber-500/15 dark:text-amber-300' : 'border border-stone-300 dark:border-stone-700'"
        @click="toggleDryRun"
      >
        {{ app.dryRun ? t.settings.on : t.settings.off }}
      </button>
    </div>

    <div class="mt-3 flex items-center gap-4 rounded-lg border border-stone-200 bg-white p-4 dark:border-stone-800 dark:bg-stone-900">
      <div class="flex-1">
        <h2 class="font-medium">{{ t.settings.xmp.title }}</h2>
        <p class="mt-0.5 text-stone-500 dark:text-stone-400">
          {{ t.settings.xmp.hint }}
        </p>
      </div>
      <button
        class="rounded-md px-3 py-1.5"
        :class="xmp ? 'bg-stone-900 text-white dark:bg-stone-100 dark:text-stone-900' : 'border border-stone-300 dark:border-stone-700'"
        @click="toggleXmp"
      >
        {{ xmp ? t.settings.on : t.settings.off }}
      </button>
    </div>

    <h2 class="mt-8 mb-3 text-xs font-medium tracking-wide text-stone-500 uppercase dark:text-stone-400">{{ t.settings.maintenance }}</h2>
    <div class="divide-y divide-stone-100 rounded-lg border border-stone-200 bg-white dark:divide-stone-800 dark:border-stone-800 dark:bg-stone-900">
      <div class="flex items-center gap-4 p-4">
        <div class="flex-1">
          <h3 class="font-medium">{{ t.settings.emptyDirs.title }}</h3>
          <p class="mt-0.5 text-stone-500 dark:text-stone-400">
            {{ t.settings.emptyDirs.hint }}
          </p>
        </div>
        <button class="shrink-0 rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="emptyDirs">{{ t.settings.emptyDirs.search }}</button>
      </div>
      <div v-if="cache" class="flex items-center gap-4 p-4">
        <div class="flex-1">
          <h3 class="font-medium">{{ t.settings.cache.title }}</h3>
          <p class="mt-0.5 text-stone-500 tabular-nums dark:text-stone-400">{{ t.settings.cache.sizes(formatBytes(cache.thumbs), formatBytes(cache.previews), formatBytes(cache.logs)) }}</p>
        </div>
        <button class="shrink-0 rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="clear('previews')">{{ t.settings.cache.clearCull }}</button>
        <button class="shrink-0 rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="clear('thumbs')">{{ t.settings.cache.clearGrid }}</button>
      </div>
      <div v-if="cache" class="flex items-center gap-4 p-4">
        <div class="min-w-0 flex-1">
          <h3 class="font-medium">{{ t.settings.dataDir.title }}</h3>
          <p class="mt-0.5 truncate font-mono text-[11px] text-stone-500 select-text" :title="cache.data_dir">{{ cache.data_dir }}</p>
        </div>
        <button class="shrink-0 rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="openDataDir">{{ t.settings.dataDir.show }}</button>
      </div>
    </div>

    <h2 class="mt-8 mb-3 text-xs font-medium tracking-wide text-stone-500 uppercase dark:text-stone-400">{{ t.settings.systemStatus }}</h2>
    <SystemStatus />
    <p class="mt-4 text-xs text-stone-400">
      {{ t.settings.credits }}
    </p>
  </section>
</template>
