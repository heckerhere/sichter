import type { ScanProgress } from "./api";
import { t } from "./i18n";

type Phase = ScanProgress["phase"];

/** Reihenfolge, Beschriftung und grobe Gewichtung der Scan-Phasen. */
export const PHASES: { id: Phase; readonly label: string; weight: number }[] = (
  [
    ["crawl", 5],
    ["detect", 5],
    ["meta", 25],
    ["group", 5],
    ["hash", 25],
    ["thumbs", 35],
    ["done", 0],
  ] as const
).map(([id, weight]) => ({
  id,
  weight,
  // Getter: Beschriftung immer in der aktuellen Sprache.
  get label() {
    return t.app.scan.phases[id];
  },
}));

const TOTAL_WEIGHT = PHASES.reduce((s, p) => s + p.weight, 0);

export function phaseLabel(phase: Phase | undefined): string {
  return PHASES.find((p) => p.id === phase)?.label ?? t.app.scan.starting;
}

/** Schritt n von m (1-basiert, ohne "done"). */
export function phaseStep(phase: Phase | undefined): { step: number; of: number } {
  const of = PHASES.length - 1;
  const index = PHASES.findIndex((p) => p.id === phase);
  return { step: Math.min(of, Math.max(1, index + 1)), of };
}

/** Gesamtfortschritt 0–100 über alle Phasen. Unbekannte Gesamtzahl zählt als 0 % der Phase. */
export function overallPercent(p: ScanProgress | null): number {
  if (!p) return 0;
  if (p.phase === "done") return 100;
  let before = 0;
  for (const phase of PHASES) {
    if (phase.id === p.phase) {
      const fraction = p.total > 0 ? Math.min(1, p.done / p.total) : 0;
      return Math.round(((before + phase.weight * fraction) / TOTAL_WEIGHT) * 100);
    }
    before += phase.weight;
  }
  return 0;
}
