<script setup lang="ts">
// Plan-Vorschau, Probelauf, Ausführung und Undo (Sicherheitsmodell 3.2, 3.3, 3.7).
import { computed, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { ask } from "@tauri-apps/plugin-dialog";
import { api, type ConflictKind, type RunDetails, type RunReport } from "../api";
import { useAppStore } from "../stores/app";
import { useRunsStore } from "../stores/runs";
import { formatBytes, formatInt } from "../format";

const route = useRoute();
const router = useRouter();
const app = useAppStore();
const runs = useRunsStore();

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
  },
);

const running = computed(() => runs.state?.running === runId.value);
const progress = computed(() => (runs.progress?.run_id === runId.value ? runs.progress : null));
const isTrash = computed(() => details.value?.kind.endsWith("to_trash") ?? false);
const trashFiles = computed(() => details.value?.ops.filter((o) => o.op === "trash").map((o) => o.src!) ?? []);
const s = computed(() => details.value?.summary);
const spaceProblem = computed(() => s.value && s.value.free_bytes !== null && s.value.bytes_to_copy > s.value.free_bytes);

const conflictLabel: Record<ConflictKind, string> = {
  duplicate_at_target: "Liegt identisch schon am Ziel, bleibt an der Quelle",
  renamed: "Name am Ziel belegt, mit (2), (3) … umbenannt",
  not_hashed: "Noch nicht vollständig gescannt, übersprungen",
  group_partial: "Teil einer RAW+JPEG-Gruppe, die nicht ganz doppelt ist, übersprungen",
};
const conflictCounts = computed(() => {
  const m = new Map<ConflictKind, number>();
  for (const c of s.value?.conflicts ?? []) m.set(c.kind, (m.get(c.kind) ?? 0) + 1);
  return [...m.entries()];
});

/** Gemeinsamer Pfadanfang, damit Bäume kurz bleiben. */
const prefix = computed(() => {
  const paths = [...Object.keys(details.value?.before ?? {}), ...Object.keys(details.value?.after ?? {})];
  if (!paths.length) return "";
  let p = paths[0];
  for (const x of paths) while (!x.startsWith(p)) p = p.slice(0, p.lastIndexOf("/"));
  return p;
});
const rel = (path: string | null) => (path ? path.slice(prefix.value.length).replace(/^\//, "") || "." : "");
const tree = (map: Record<string, number>) =>
  Object.entries(map)
    .map(([dir, n]) => ({ dir: rel(dir), n }))
    .sort((a, b) => a.dir.localeCompare(b.dir, "de", { numeric: true }));

const opLabel = { mkdir: "Ordner anlegen", move: "Verschieben", copy: "Kopieren", trash: "Papierkorb" };
const statusLabel = { planned: "geplant", running: "läuft", done: "ausgeführt", aborted: "abgebrochen", undone: "rückgängig gemacht" };

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
  const verb = d.kind === "reorganize" && d.title.includes("Kopieren") ? "kopiert" : "verschoben";
  const ok = await ask(
    `${formatInt(d.files)} Dateien (${formatBytes(s.value?.bytes ?? 0)}) werden ${verb}, ${formatInt(d.mkdirs)} Ordner angelegt. Es wird nichts überschrieben. Der Lauf kann danach rückgängig gemacht werden.`,
    { title: "Plan ausführen", kind: "warning", okLabel: `${formatInt(d.files)} Dateien ${verb === "kopiert" ? "kopieren" : "verschieben"}`, cancelLabel: "Abbrechen" },
  );
  if (ok) await act(() => api.runExecute(runId.value, d.files));
}

async function confirmTrash() {
  showTrashConfirm.value = false;
  await act(() => api.runExecute(runId.value, details.value!.files));
}

async function undo() {
  const ok = await ask(
    "Alle Dateien dieses Laufs werden an ihren vorherigen Ort zurückgebracht, angelegte Ordner werden entfernt, wenn sie leer sind. Kopien landen im Papierkorb. Dateien im Papierkorb musst du selbst über „Zurücklegen“ wiederherstellen.",
    { title: "Lauf rückgängig machen", kind: "warning", okLabel: "Rückgängig machen", cancelLabel: "Abbrechen" },
  );
  if (ok) await act(() => api.runUndo(runId.value));
}

async function discard() {
  await api.runDiscard(runId.value);
  await runs.refresh();
  await router.push("/verlauf");
}

async function disableDryRun() {
  const ok = await ask("Danach kannst du Pläne wirklich ausführen. Jede Ausführung fragt weiterhin vorher nach.", {
    title: "Dry-Run-Modus ausschalten",
    okLabel: "Ausschalten",
    cancelLabel: "Abbrechen",
  });
  if (ok) await app.setDryRun(false);
}
</script>

<template>
  <section class="mx-auto max-w-5xl px-8 py-8">
    <RouterLink to="/verlauf" class="text-xs text-stone-500 hover:text-stone-800 dark:hover:text-stone-200">← Verlauf</RouterLink>
    <p v-if="error" class="mt-3 rounded-md bg-red-50 px-3 py-2 text-red-700 dark:bg-red-500/10 dark:text-red-300">{{ error }}</p>

    <template v-if="details && s">
      <header class="mt-2 flex flex-wrap items-center gap-3">
        <h1 class="text-xl font-semibold tracking-tight">{{ details.title }}</h1>
        <span class="rounded bg-stone-100 px-2 py-0.5 text-xs dark:bg-stone-800">{{ statusLabel[details.status] }}</span>
        <span class="text-xs text-stone-400">Lauf {{ details.id }} · erstellt {{ details.created_at.slice(0, 16).replace("T", " ") }} UTC</span>
      </header>

      <!-- Dry-Run-Hinweis -->
      <div
        v-if="app.dryRun && (details.status === 'planned' || details.status === 'aborted')"
        class="mt-4 flex items-center gap-3 rounded-lg border border-amber-200 bg-amber-50 px-4 py-3 text-amber-900 dark:border-amber-900 dark:bg-amber-950 dark:text-amber-200"
      >
        <span class="flex-1">Dry-Run-Modus ist aktiv: Du kannst einen Probelauf machen, es wird nichts verändert.</span>
        <button class="rounded-md border border-amber-300 px-2.5 py-1 text-xs hover:bg-amber-100 dark:border-amber-800 dark:hover:bg-amber-900" @click="disableDryRun">
          Zum Ausführen ausschalten
        </button>
      </div>

      <!-- Zähler -->
      <dl class="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-4">
        <div class="rounded-lg border border-stone-200 bg-white p-3 dark:border-stone-800 dark:bg-stone-900">
          <dt class="text-xs text-stone-500">Dateien</dt>
          <dd class="text-lg font-semibold tabular-nums">{{ formatInt(details.files) }}</dd>
          <dd class="text-[11px] text-stone-400 tabular-nums">{{ formatBytes(s.bytes) }} · {{ formatInt(s.groups) }} Einheiten</dd>
        </div>
        <div class="rounded-lg border border-stone-200 bg-white p-3 dark:border-stone-800 dark:bg-stone-900">
          <dt class="text-xs text-stone-500">Neue Ordner</dt>
          <dd class="text-lg font-semibold tabular-nums">{{ formatInt(details.mkdirs) }}</dd>
        </div>
        <div class="rounded-lg border border-stone-200 bg-white p-3 dark:border-stone-800 dark:bg-stone-900">
          <dt class="text-xs text-stone-500">Schon am Ziel / übersprungen</dt>
          <dd class="text-lg font-semibold tabular-nums">{{ formatInt(s.in_place) }} / {{ formatInt(s.skipped_unconfirmed + s.conflicts.filter((c) => c.kind !== "renamed").length) }}</dd>
        </div>
        <div class="rounded-lg border bg-white p-3 dark:bg-stone-900" :class="spaceProblem ? 'border-red-300 dark:border-red-800' : 'border-stone-200 dark:border-stone-800'">
          <dt class="text-xs text-stone-500">Speicherplatz am Ziel</dt>
          <dd class="text-lg font-semibold tabular-nums">{{ s.bytes_to_copy ? formatBytes(s.bytes_to_copy) : "kein Kopieren" }}</dd>
          <dd class="text-[11px] tabular-nums" :class="spaceProblem ? 'text-red-600' : 'text-stone-400'">
            <template v-if="s.free_bytes !== null">{{ formatBytes(s.free_bytes) }} frei{{ spaceProblem ? " – reicht nicht" : "" }}</template>
            <template v-else>Verschieben auf demselben Laufwerk</template>
          </dd>
        </div>
      </dl>

      <!-- Konflikte -->
      <div v-if="s.conflicts.length" class="mt-4 rounded-lg border border-stone-200 bg-white px-4 py-3 dark:border-stone-800 dark:bg-stone-900">
        <button class="flex w-full items-center gap-2 text-left" @click="showConflicts = !showConflicts">
          <span class="font-medium">{{ s.conflicts.length }} Hinweise</span>
          <span class="text-xs text-stone-500">{{ conflictCounts.map(([k, n]) => `${n} × ${conflictLabel[k]}`).join(" · ") }}</span>
        </button>
        <ul v-if="showConflicts" class="mt-2 max-h-56 space-y-1 overflow-y-auto font-mono text-[11px]">
          <li v-for="(c, i) in s.conflicts" :key="i" class="grid grid-cols-[1fr_auto_1fr] gap-2">
            <span class="truncate" :title="c.path">{{ rel(c.path) }}</span>
            <span class="text-stone-400">{{ c.kind === "renamed" ? "→" : c.kind === "duplicate_at_target" ? "=" : "·" }}</span>
            <span class="truncate text-stone-500" :title="c.target ?? ''">{{ c.target ? rel(c.target) : conflictLabel[c.kind] }}</span>
          </li>
        </ul>
      </div>

      <!-- Vorher / Nachher -->
      <div v-if="!isTrash" class="mt-4 grid gap-4 md:grid-cols-2">
        <div v-for="(side, i) in [{ label: 'Vorher: Dateien verlassen', map: details.before }, { label: 'Nachher: Dateien kommen an', map: details.after }]" :key="i" class="rounded-lg border border-stone-200 bg-white dark:border-stone-800 dark:bg-stone-900">
          <h2 class="border-b border-stone-100 px-4 py-2 text-xs font-medium text-stone-500 dark:border-stone-800">{{ side.label }}</h2>
          <ul class="max-h-64 overflow-y-auto px-4 py-2 font-mono text-[11px]">
            <li v-for="t in tree(side.map)" :key="t.dir" class="flex gap-2 py-0.5">
              <span class="min-w-0 flex-1 truncate" :title="t.dir">{{ t.dir }}</span>
              <span class="text-stone-400 tabular-nums">{{ formatInt(t.n) }}</span>
            </li>
          </ul>
        </div>
      </div>
      <p class="mt-2 text-[11px] text-stone-400">Pfade relativ zu {{ prefix || "/" }}</p>

      <!-- Fortschritt -->
      <div v-if="running" class="mt-4 rounded-lg border border-sky-200 bg-sky-50 px-4 py-3 dark:border-sky-900 dark:bg-sky-950">
        <div class="flex items-center gap-3">
          <span class="font-medium">{{ runs.state?.paused ? "Pausiert" : "Läuft" }}</span>
          <span v-if="progress" class="tabular-nums">{{ formatInt(progress.done) }} / {{ formatInt(progress.total) }}</span>
          <span class="min-w-0 flex-1 truncate text-xs text-stone-500">{{ progress?.current }}</span>
          <button class="rounded-md border border-stone-300 px-2.5 py-1 text-xs dark:border-stone-700" @click="togglePause">
            {{ runs.state?.paused ? "Fortsetzen" : "Pause" }}
          </button>
          <button class="rounded-md border border-stone-300 px-2.5 py-1 text-xs dark:border-stone-700" @click="cancel">Abbrechen</button>
        </div>
        <div class="mt-2 h-1.5 overflow-hidden rounded-full bg-sky-100 dark:bg-sky-900">
          <div class="h-full rounded-full bg-sky-500 transition-[width]" :style="{ width: `${progress && progress.total ? (progress.done / progress.total) * 100 : 2}%` }" />
        </div>
        <p class="mt-1.5 text-[11px] text-stone-500">Abbrechen hält nach der laufenden Gruppe an. Der Lauf kann danach fortgesetzt oder rückgängig gemacht werden.</p>
      </div>

      <!-- Ergebnis -->
      <div v-if="report" class="mt-4 rounded-lg border px-4 py-3" :class="report.report.failed ? 'border-red-200 bg-red-50 dark:border-red-900 dark:bg-red-950' : 'border-emerald-200 bg-emerald-50 dark:border-emerald-900 dark:bg-emerald-950'">
        <p class="font-medium">
          <template v-if="report.action === 'dry'">Probelauf: {{ report.report.done }} Dateien bereit, {{ report.report.failed }} Probleme. Es wurde nichts verändert.</template>
          <template v-else-if="report.action === 'undo'">Rückgängig: {{ report.report.undone }} Dateien zurück, {{ report.report.failed }} Probleme.</template>
          <template v-else>{{ report.report.cancelled ? "Abgebrochen" : "Fertig" }}: {{ report.report.done }} Dateien erledigt, {{ report.report.failed }} fehlgeschlagen<template v-if="report.report.undone">, {{ report.report.undone }} zur Wahrung zusammengehöriger Dateien zurückgesetzt</template>.</template>
        </p>
        <ul v-if="report.report.messages.length" class="mt-2 max-h-40 overflow-y-auto font-mono text-[11px]">
          <li v-for="([p, m], i) in report.report.messages" :key="i">{{ rel(p) || p }}: {{ m }}</li>
        </ul>
        <div v-if="report.report.manual_restore.length" class="mt-2 text-xs">
          {{ report.report.manual_restore.length }} Dateien liegen im Papierkorb. Öffne den Papierkorb und wähle „Zurücklegen“, um sie wiederherzustellen.
        </div>
      </div>

      <!-- Aktionen -->
      <div class="mt-4 flex flex-wrap items-center gap-2">
        <template v-if="!running && (details.status === 'planned' || details.status === 'aborted')">
          <button class="rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="dry">Probelauf</button>
          <button
            :disabled="app.dryRun || !!spaceProblem"
            class="rounded-md bg-stone-900 px-3 py-1.5 text-white hover:bg-stone-700 disabled:opacity-40 dark:bg-stone-100 dark:text-stone-900"
            :title="app.dryRun ? 'Dry-Run-Modus ist aktiv' : ''"
            @click="execute"
          >
            {{ details.status === "aborted" ? "Fortsetzen …" : "Ausführen …" }}
          </button>
          <button v-if="details.status === 'aborted'" class="rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="undo">Rückgängig machen …</button>
          <button v-if="details.status === 'planned'" class="ml-auto text-stone-500 hover:text-red-600" @click="discard">Plan verwerfen</button>
        </template>
        <template v-else-if="!running && details.status === 'done'">
          <button class="rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800" @click="undo">Rückgängig machen …</button>
        </template>
        <button v-if="details.log_path" class="text-stone-500 underline-offset-2 hover:underline" @click="openLog">Protokoll öffnen</button>
      </div>

      <!-- Operationen -->
      <div class="mt-6 overflow-hidden rounded-lg border border-stone-200 bg-white dark:border-stone-800 dark:bg-stone-900">
        <h2 class="border-b border-stone-100 px-4 py-2 text-xs font-medium text-stone-500 dark:border-stone-800">
          Operationen ({{ formatInt(details.total_ops) }}<template v-if="details.total_ops > details.ops.length">, die ersten {{ details.ops.length }}; vollständig im Protokoll</template>)
        </h2>
        <table class="w-full table-fixed text-left font-mono text-[11px]">
          <tbody>
            <tr v-for="o in details.ops" :key="o.seq" class="border-b border-stone-50 last:border-0 dark:border-stone-800/60" :class="o.status === 'failed' && 'bg-red-50 dark:bg-red-950/40'">
              <td class="w-24 px-3 py-1 font-sans text-stone-500">{{ opLabel[o.op] }}</td>
              <td class="truncate px-3 py-1" :title="o.src ?? ''">{{ rel(o.src) }}</td>
              <td class="w-4 text-stone-400">{{ o.dst && o.src ? "→" : "" }}</td>
              <td class="truncate px-3 py-1" :title="o.dst ?? o.keep_path ?? ''">{{ o.dst ? rel(o.dst) : o.keep_path ? `bleibt: ${rel(o.keep_path)}` : "" }}</td>
              <td class="w-28 px-3 py-1 font-sans" :class="o.status === 'failed' ? 'text-red-600' : 'text-stone-400'" :title="o.error ?? ''">{{ o.error ?? o.status }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </template>

    <!-- Papierkorb-Bestätigung mit Dateiliste -->
    <div v-if="showTrashConfirm && details" class="fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-8">
      <div class="flex max-h-full w-full max-w-2xl flex-col rounded-xl bg-white p-5 shadow-xl dark:bg-stone-900">
        <h2 class="text-lg font-semibold">{{ formatInt(trashFiles.length) }} Dateien in den Papierkorb legen?</h2>
        <p class="mt-1 text-stone-500">
          Die Dateien landen im Papierkorb des Systems und lassen sich dort über „Zurücklegen“ wiederherstellen. Bei Duplikaten prüft Sichter direkt vorher, dass die
          bleibende Kopie unverändert vorhanden ist.
        </p>
        <ul class="mt-3 min-h-0 flex-1 overflow-y-auto rounded-md bg-stone-50 p-2 font-mono text-[11px] dark:bg-stone-950">
          <li v-for="f in trashFiles" :key="f" class="truncate" :title="f">{{ f }}</li>
        </ul>
        <div class="mt-4 flex justify-end gap-2">
          <button class="rounded-md border border-stone-300 px-3 py-1.5 dark:border-stone-700" @click="showTrashConfirm = false">Abbrechen</button>
          <button class="rounded-md bg-red-600 px-3 py-1.5 text-white hover:bg-red-700" @click="confirmTrash">{{ formatInt(trashFiles.length) }} Dateien in den Papierkorb</button>
        </div>
      </div>
    </div>
  </section>
</template>
