<script setup lang="ts">
// Plan-Vorschau, Probelauf, Ausführung und Undo (Sicherheitsmodell 3.2, 3.3, 3.7).
import { computed, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { ask } from "@tauri-apps/plugin-dialog";
import { api, type ConflictKind, type RunDetails, type RunReport } from "../api";
import { useAppStore } from "../stores/app";
import { useRunsStore } from "../stores/runs";
import { useLibraryStore } from "../stores/library";
import { formatBytes, formatInt } from "../format";
import { t } from "../i18n";

const route = useRoute();
const router = useRouter();
const app = useAppStore();
const runs = useRunsStore();
const lib = useLibraryStore();

const runId = computed(() => Number(route.params.id));
const details = ref<RunDetails | null>(null);
const error = ref<string | null>(null);
const report = ref<{ action: string; report: RunReport } | null>(null);
const showTrashConfirm = ref(false);
const showConflicts = ref(false);

async function load() {
  try {
    details.value = await api.runDetails(runId.value);
    error.value = null;
  } catch (e) {
    error.value = String(e);
  }
}
watch(runId, () => {
  report.value = null;
  void load();
}, { immediate: true });

watch(
  () => runs.lastFinished,
  async (f) => {
    if (!f || f.run_id !== runId.value) return;
    if (f.status === "ok") report.value = { action: f.action, report: f.report };
    else error.value = f.message;
    await load();
    // Nach dem Import den Zielordner gleich in die Bibliothek aufnehmen.
    if (f.status === "ok" && f.action === "execute" && isImport.value && f.report.done > 0) await scanImportTarget();
  },
);

const running = computed(() => runs.state?.running === runId.value);
const progress = computed(() => (runs.progress?.run_id === runId.value ? runs.progress : null));
const isTrash = computed(() => details.value?.kind.endsWith("to_trash") ?? false);
const isImport = computed(() => details.value?.kind === "photos_import");
const importTarget = computed(() => (isImport.value ? String(details.value?.params.target ?? "") : ""));

async function scanImportTarget() {
  const errors = await lib.includeFolder(importTarget.value);
  if (errors.length) error.value = errors.join("\n");
  else await router.push("/");
}
const trashFiles = computed(() => details.value?.ops.filter((o) => o.op === "trash").map((o) => o.src!) ?? []);
const s = computed(() => details.value?.summary);
const spaceProblem = computed(() => s.value && s.value.free_bytes !== null && s.value.bytes_to_copy > s.value.free_bytes);

const conflictLabel = (k: ConflictKind) => t.plan.conflict[k];
const conflictCounts = computed(() => {
  const m = new Map<ConflictKind, number>();
  for (const c of s.value?.conflicts ?? []) m.set(c.kind, (m.get(c.kind) ?? 0) + 1);
  return [...m.entries()];
});

/** Gemeinsamer Pfadanfang, damit Bäume kurz bleiben. */
const prefix = computed(() => {
  const paths = [...(isImport.value ? [] : Object.keys(details.value?.before ?? {})), ...Object.keys(details.value?.after ?? {})];
  if (!paths.length) return "";
  let p = paths[0];
  for (const x of paths) while (!x.startsWith(p)) p = p.slice(0, p.lastIndexOf("/"));
  return p;
});
const rel = (path: string | null) => (path ? (path.startsWith(prefix.value) ? path.slice(prefix.value.length).replace(/^\//, "") || "." : path) : "");
const tree = (map: Record<string, number>) =>
  Object.entries(map)
    .map(([dir, n]) => ({ dir: rel(dir), n }))
    .sort((a, b) => a.dir.localeCompare(b.dir, "de", { numeric: true }));


async function act(f: () => Promise<unknown>) {
  error.value = null;
  report.value = null;
  try {
    await f();
    await runs.refresh();
  } catch (e) {
    error.value = String(e);
  }
}

const dry = () => act(() => api.runDry(runId.value));
const togglePause = () => act(() => api.runPause(!runs.state?.paused));
const cancel = () => api.runCancel();
const openLog = () => api.runOpenLog(runId.value).catch((e) => (error.value = String(e)));

async function execute() {
  const d = details.value!;
  if (isTrash.value) {
    showTrashConfirm.value = true;
    return;
  }
  if (isImport.value) {
    const ok = await ask(t.plan.dialogs.importMessage(formatInt(d.files), formatBytes(s.value?.bytes ?? 0), importTarget.value, formatInt(d.mkdirs)), {
      title: t.plan.dialogs.importTitle,
      kind: "warning",
      okLabel: t.plan.dialogs.importOk(formatInt(d.files)),
      cancelLabel: t.common.cancel,
    });
    if (ok) await act(() => api.runExecute(runId.value, d.files));
    return;
  }
  // Modus aus den gespeicherten Parametern; Titel als Rückfall für ältere Läufe.
  const copy = d.kind === "reorganize" && (d.params.mode === "copy" || d.title.includes("Kopieren"));
  const ok = await ask(t.plan.dialogs.executeMessage(formatInt(d.files), formatBytes(s.value?.bytes ?? 0), copy, formatInt(d.mkdirs)), {
    title: t.plan.dialogs.executeTitle,
    kind: "warning",
    okLabel: t.plan.dialogs.executeOk(formatInt(d.files), copy),
    cancelLabel: t.common.cancel,
  });
  if (ok) await act(() => api.runExecute(runId.value, d.files));
}

async function confirmTrash() {
  showTrashConfirm.value = false;
  await act(() => api.runExecute(runId.value, details.value!.files));
}

async function undo() {
  const ok = await ask(t.plan.dialogs.undoMessage, {
    title: t.plan.dialogs.undoTitle,
    kind: "warning",
    okLabel: t.plan.dialogs.undoOk,
    cancelLabel: t.common.cancel,
  });
  if (ok) await act(() => api.runUndo(runId.value));
}

async function discard() {
  await api.runDiscard(runId.value);
  await runs.refresh();
  await router.push("/verlauf");
}

async function disableDryRun() {
  const ok = await ask(t.plan.dialogs.dryRunMessage, {
    title: t.plan.dialogs.dryRunTitle,
    okLabel: t.plan.dialogs.dryRunOk,
    cancelLabel: t.common.cancel,
  });
  if (ok) await app.setDryRun(false);
}
</script>

<template>
  <section class="mx-auto max-w-5xl px-8 py-8">
    <RouterLink to="/verlauf" class="text-xs text-stone-500 hover:text-stone-800 dark:hover:text-stone-200">{{ t.plan.back }}</RouterLink>
    <p v-if="error" class="mt-3 rounded-md bg-red-50 px-3 py-2 text-red-700 dark:bg-red-500/10 dark:text-red-300">{{ error }}</p>

    <template v-if="details && s">
      <header class="mt-2 flex flex-wrap items-center gap-3">
        <h1 class="text-xl font-semibold tracking-tight">{{ details.title }}</h1>
        <span class="rounded bg-stone-100 px-2 py-0.5 text-xs dark:bg-stone-800">{{ t.plan.runStatus[details.status] }}</span>
        <span class="text-xs text-stone-400">{{ t.plan.meta(details.id, details.created_at.slice(0, 16).replace("T", " ")) }}</span>
      </header>

      <!-- Dry-Run-Hinweis -->
      <div
        v-if="app.dryRun && (details.status === 'planned' || details.status === 'aborted')"
        class="mt-4 flex items-center gap-3 rounded-lg border border-amber-200 bg-amber-50 px-4 py-3 text-amber-900 dark:border-amber-900 dark:bg-amber-950 dark:text-amber-200"
      >
        <span class="flex-1">{{ t.plan.dryRunActive }}</span>
        <button class="rounded-md border border-amber-300 px-2.5 py-1 text-xs hover:bg-amber-100 dark:border-amber-800 dark:hover:bg-amber-900" @click="disableDryRun">
          {{ t.plan.dryRunDisableButton }}
        </button>
      </div>

      <!-- Zähler -->
      <dl class="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-4">
        <div class="rounded-lg border border-stone-200 bg-white p-3 dark:border-stone-800 dark:bg-stone-900">
          <dt class="text-xs text-stone-500">{{ t.plan.counters.files }}</dt>
          <dd class="text-lg font-semibold tabular-nums">{{ formatInt(details.files) }}</dd>
          <dd class="text-[11px] text-stone-400 tabular-nums">{{ t.plan.counters.units(formatBytes(s.bytes), formatInt(s.groups)) }}</dd>
        </div>
        <div class="rounded-lg border border-stone-200 bg-white p-3 dark:border-stone-800 dark:bg-stone-900">
          <dt class="text-xs text-stone-500">{{ t.plan.counters.newFolders }}</dt>
          <dd class="text-lg font-semibold tabular-nums">{{ formatInt(details.mkdirs) }}</dd>
        </div>
        <div v-if="isImport" class="rounded-lg border border-stone-200 bg-white p-3 dark:border-stone-800 dark:bg-stone-900">
          <dt class="text-xs text-stone-500">{{ t.plan.counters.alreadyImported }}</dt>
          <dd class="text-lg font-semibold tabular-nums">{{ formatInt(s.already_imported ?? 0) }} / {{ formatInt(s.not_exportable ?? 0) }}</dd>
        </div>
        <div v-else class="rounded-lg border border-stone-200 bg-white p-3 dark:border-stone-800 dark:bg-stone-900">
          <dt class="text-xs text-stone-500">{{ t.plan.counters.inPlaceSkipped }}</dt>
          <dd class="text-lg font-semibold tabular-nums">{{ formatInt(s.in_place) }} / {{ formatInt(s.skipped_unconfirmed + s.conflicts.filter((c) => c.kind !== "renamed").length) }}</dd>
        </div>
        <div class="rounded-lg border bg-white p-3 dark:bg-stone-900" :class="spaceProblem ? 'border-red-300 dark:border-red-800' : 'border-stone-200 dark:border-stone-800'">
          <dt class="text-xs text-stone-500">{{ t.plan.counters.space }}</dt>
          <dd class="text-lg font-semibold tabular-nums">{{ s.bytes_to_copy ? formatBytes(s.bytes_to_copy) : t.plan.counters.noCopy }}</dd>
          <dd class="text-[11px] tabular-nums" :class="spaceProblem ? 'text-red-600' : 'text-stone-400'">
            <template v-if="s.free_bytes !== null">{{ t.plan.counters.free(formatBytes(s.free_bytes), !!spaceProblem) }}</template>
            <template v-else>{{ t.plan.counters.sameDrive }}</template>
          </dd>
        </div>
      </dl>

      <!-- Konflikte -->
      <div v-if="s.conflicts.length" class="mt-4 rounded-lg border border-stone-200 bg-white px-4 py-3 dark:border-stone-800 dark:bg-stone-900">
        <button class="flex w-full items-center gap-2 text-left" @click="showConflicts = !showConflicts">
          <span class="font-medium">{{ t.plan.notes(s.conflicts.length) }}</span>
          <span class="text-xs text-stone-500">{{ conflictCounts.map(([k, n]) => `${n} × ${conflictLabel(k)}`).join(" · ") }}</span>
        </button>
        <ul v-if="showConflicts" class="mt-2 max-h-56 space-y-1 overflow-y-auto font-mono text-[11px]">
          <li v-for="(c, i) in s.conflicts" :key="i" class="grid grid-cols-[1fr_auto_1fr] gap-2">
            <span class="truncate" :title="c.path">{{ rel(c.path) }}</span>
            <span class="text-stone-400">{{ c.kind === "renamed" ? "→" : c.kind === "duplicate_at_target" ? "=" : "·" }}</span>
            <span class="truncate text-stone-500" :title="c.target ?? ''">{{ c.target ? rel(c.target) : conflictLabel(c.kind) }}</span>
          </li>
        </ul>
      </div>

      <!-- Vorher / Nachher -->
      <div v-if="!isTrash" class="mt-4 grid gap-4" :class="!isImport && 'md:grid-cols-2'">
        <div
          v-for="(side, i) in isImport ? [{ label: t.plan.sides.fromPhotos, map: details.after }] : [{ label: t.plan.sides.before, map: details.before }, { label: t.plan.sides.after, map: details.after }]"
          :key="i" class="rounded-lg border border-stone-200 bg-white dark:border-stone-800 dark:bg-stone-900">
          <h2 class="border-b border-stone-100 px-4 py-2 text-xs font-medium text-stone-500 dark:border-stone-800">{{ side.label }}</h2>
          <ul class="max-h-64 overflow-y-auto px-4 py-2 font-mono text-[11px]">
            <li v-for="row in tree(side.map)" :key="row.dir" class="flex gap-2 py-0.5">
              <span class="min-w-0 flex-1 truncate" :title="row.dir">{{ row.dir }}</span>
              <span class="text-stone-400 tabular-nums">{{ formatInt(row.n) }}</span>
            </li>
          </ul>
        </div>
      </div>
      <p class="mt-2 text-[11px] text-stone-400">{{ t.plan.relativeTo(prefix || "/") }}</p>

      <!-- Fortschritt -->
      <div v-if="running" class="mt-4 rounded-lg border border-sky-200 bg-sky-50 px-4 py-3 dark:border-sky-900 dark:bg-sky-950">
        <div class="flex items-center gap-3">
          <span class="font-medium">{{ runs.state?.paused ? t.plan.progress.paused : t.plan.progress.running }}</span>
          <span v-if="progress" class="tabular-nums">{{ formatInt(progress.done) }} / {{ formatInt(progress.total) }}</span>
          <span class="min-w-0 flex-1 truncate text-xs text-stone-500">{{ progress?.current }}</span>
          <button class="rounded-md border border-stone-300 px-2.5 py-1 text-xs dark:border-stone-700" @click="togglePause">
            {{ runs.state?.paused ? t.plan.progress.resume : t.plan.progress.pause }}
          </button>
          <button class="rounded-md border border-stone-300 px-2.5 py-1 text-xs dark:border-stone-700" @click="cancel">{{ t.common.cancel }}</button>
        </div>
        <div class="mt-2 h-1.5 overflow-hidden rounded-full bg-sky-100 dark:bg-sky-900">
          <div class="h-full rounded-full bg-sky-500 transition-[width]" :style="{ width: `${progress && progress.total ? (progress.done / progress.total) * 100 : 2}%` }" />
        </div>
        <p class="mt-1.5 text-[11px] text-stone-500">{{ t.plan.progress.cancelHint }}</p>
      </div>

      <!-- Ergebnis -->
      <div v-if="report" class="mt-4 rounded-lg border px-4 py-3" :class="report.report.failed ? 'border-red-200 bg-red-50 dark:border-red-900 dark:bg-red-950' : 'border-emerald-200 bg-emerald-50 dark:border-emerald-900 dark:bg-emerald-950'">
        <p class="font-medium">
          <template v-if="report.action === 'dry'">{{ t.plan.result.dry(report.report.done, report.report.failed) }}</template>
          <template v-else-if="report.action === 'undo'">{{ t.plan.result.undo(report.report.undone, report.report.failed) }}</template>
          <template v-else>{{ t.plan.result.executed(report.report.cancelled, report.report.done, report.report.failed, report.report.undone) }}</template>
        </p>
        <ul v-if="report.report.messages.length" class="mt-2 max-h-40 overflow-y-auto font-mono text-[11px]">
          <li v-for="([p, m], i) in report.report.messages" :key="i">{{ rel(p) || p }}: {{ m }}</li>
        </ul>
        <div v-if="report.report.manual_restore.length" class="mt-2 text-xs">
          {{ t.plan.result.manualRestore(report.report.manual_restore.length) }}
        </div>
      </div>

      <!-- Aktionen -->
      <div class="mt-4 flex flex-wrap items-center gap-2">
        <template v-if="!running && (details.status === 'planned' || details.status === 'aborted')">
          <button class="rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="dry">{{ t.plan.actions.dry }}</button>
          <button
            :disabled="app.dryRun || !!spaceProblem"
            class="rounded-md bg-stone-900 px-3 py-1.5 text-white hover:bg-stone-700 disabled:opacity-40 dark:bg-stone-100 dark:text-stone-900"
            :title="app.dryRun ? t.plan.actions.dryRunActive : ''"
            @click="execute"
          >
            {{ details.status === "aborted" ? t.plan.actions.resume : t.plan.actions.execute }}
          </button>
          <button v-if="details.status === 'aborted'" class="rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="undo">{{ t.plan.actions.undo }}</button>
          <button v-if="details.status === 'planned'" class="ml-auto text-stone-500 hover:text-red-600" @click="discard">{{ t.plan.actions.discard }}</button>
        </template>
        <template v-else-if="!running && details.status === 'done'">
          <button
            v-if="isImport"
            :disabled="lib.scanning"
            class="rounded-md bg-stone-900 px-3 py-1.5 text-white hover:bg-stone-700 disabled:opacity-40 dark:bg-stone-100 dark:text-stone-900"
            @click="scanImportTarget"
          >
            {{ t.plan.actions.scanTarget }}
          </button>
          <button class="rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="undo">{{ t.plan.actions.undo }}</button>
        </template>
        <button v-if="details.log_path" class="text-stone-500 underline-offset-2 hover:underline" @click="openLog">{{ t.plan.actions.openLog }}</button>
      </div>

      <!-- Operationen -->
      <div class="mt-6 overflow-hidden rounded-lg border border-stone-200 bg-white dark:border-stone-800 dark:bg-stone-900">
        <h2 class="border-b border-stone-100 px-4 py-2 text-xs font-medium text-stone-500 dark:border-stone-800">
          {{ t.plan.ops.title(formatInt(details.total_ops)) }}<template v-if="details.total_ops > details.ops.length">{{ t.plan.ops.firstN(details.ops.length) }}</template>)
        </h2>
        <table class="w-full table-fixed text-left font-mono text-[11px]">
          <tbody>
            <tr v-for="o in details.ops" :key="o.seq" class="border-b border-stone-50 last:border-0 dark:border-stone-800/60" :class="o.status === 'failed' && 'bg-red-50 dark:bg-red-950/40'">
              <td class="w-24 px-3 py-1 font-sans text-stone-500">{{ t.plan.opLabel[o.op] }}</td>
              <td class="truncate px-3 py-1" :title="o.src ?? ''">{{ rel(o.src) }}</td>
              <td class="w-4 text-stone-400">{{ o.dst && o.src ? "→" : "" }}</td>
              <td class="truncate px-3 py-1" :title="o.dst ?? o.keep_path ?? ''">{{ o.dst ? rel(o.dst) : o.keep_path ? t.plan.ops.keeps(rel(o.keep_path)) : "" }}</td>
              <td class="w-28 px-3 py-1 font-sans" :class="o.status === 'failed' ? 'text-red-600' : 'text-stone-400'" :title="o.error ?? ''">{{ o.error ?? t.plan.opStatus[o.status] }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </template>

    <!-- Papierkorb-Bestätigung mit Dateiliste -->
    <div v-if="showTrashConfirm && details" class="fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-8">
      <div class="flex max-h-full w-full max-w-2xl flex-col rounded-xl bg-white p-5 shadow-xl dark:bg-stone-900">
        <h2 class="text-lg font-semibold">{{ t.plan.trash.title(formatInt(trashFiles.length)) }}</h2>
        <p class="mt-1 text-stone-500">
          {{ t.plan.trash.body }}
        </p>
        <ul class="mt-3 min-h-0 flex-1 overflow-y-auto rounded-md bg-stone-50 p-2 font-mono text-[11px] dark:bg-stone-950">
          <li v-for="f in trashFiles" :key="f" class="truncate" :title="f">{{ f }}</li>
        </ul>
        <div class="mt-4 flex justify-end gap-2">
          <button class="rounded-md border border-stone-300 px-3 py-1.5 dark:border-stone-700" @click="showTrashConfirm = false">{{ t.common.cancel }}</button>
          <button class="rounded-md bg-red-600 px-3 py-1.5 text-white hover:bg-red-700" @click="confirmTrash">{{ t.plan.trash.confirm(formatInt(trashFiles.length)) }}</button>
        </div>
      </div>
    </div>
  </section>
</template>
