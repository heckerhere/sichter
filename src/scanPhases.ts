import type { ScanProgress } from "./api";

type Phase = ScanProgress["phase"];

/** Reihenfolge, Beschriftung und grobe Gewichtung der Scan-Phasen. */
export const PHASES: { id: Phase; label: string; weight: number }[] = [
  { id: "crawl", label: "Dateien suchen", weight: 5 },
  { id: "detect", label: "Dateitypen prüfen", weight: 5 },
  { id: "meta", label: "Metadaten lesen", weight: 25 },
  { id: "group", label: "Zusammengehörige Dateien erkennen", weight: 5 },
  { id: "hash", label: "Prüfsummen berechnen", weight: 25 },
  { id: "thumbs", label: "Vorschaubilder erzeugen", weight: 35 },
  { id: "done", label: "Abschließen", weight: 0 },
];

const TOTAL_WEIGHT = PHASES.reduce((s, p) => s + p.weight, 0);

export function phaseLabel(phase: Phase | undefined): string {
  return PHASES.find((p) => p.id === phase)?.label ?? "Scan startet";
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
