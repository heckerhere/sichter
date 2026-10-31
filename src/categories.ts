import type { Kind } from "./api";
import { t } from "./i18n";

export type Category = "raw" | "image" | "video" | "sidecar" | "other";

/** Feste Reihenfolge = feste Farbe. Nie nach Rang umfärben. */
// Getter: Beschriftung immer in der aktuellen Sprache.
export const CATEGORIES: { id: Category; readonly label: string; readonly hint: string }[] = [
  { id: "raw", get label() { return t.storage.categories.raw; }, hint: "CR2, CR3, NEF, ARW, DNG, RAF, ORF, RW2" },
  { id: "image", get label() { return t.storage.categories.image; }, hint: "JPEG, HEIC, PNG, TIFF, WebP" },
  { id: "video", get label() { return t.storage.categories.video; }, hint: "MP4, MOV, AVI, MTS, MKV …" },
  { id: "sidecar", get label() { return t.storage.categories.sidecar; }, hint: "XMP, AAE, THM, PP3" },
  { id: "other", get label() { return t.storage.categories.other; }, get hint() { return t.storage.categories.otherHint; } },
];

export function categoryOf(kind: Kind | string): Category {
  switch (kind) {
    case "raw":
      return "raw";
    case "video":
      return "video";
    case "sidecar":
      return "sidecar";
    case "jpeg":
    case "heic":
    case "png":
    case "tiff":
    case "webp":
      return "image";
    default:
      return "other";
  }
}

export const cssVar = (c: Category) => `var(--viz-${c})`;

/** Aufgelöste Farbe für Canvas-Diagramme (ECharts kennt keine CSS-Variablen). */
export function resolvedColor(name: Category | "surface" | "track"): string {
  return getComputedStyle(document.documentElement).getPropertyValue(`--viz-${name}`).trim();
}

/** Summiert Größen pro Kategorie, in fester Kategorie-Reihenfolge. */
export function byCategory(kinds: { kind: string; size: number; count: number }[]) {
  const sums = new Map<Category, { size: number; count: number }>();
  for (const k of kinds) {
    const c = categoryOf(k.kind);
    const s = sums.get(c) ?? { size: 0, count: 0 };
    s.size += k.size;
    s.count += k.count;
    sums.set(c, s);
  }
  return CATEGORIES.map((c) => ({ ...c, size: sums.get(c.id)?.size ?? 0, count: sums.get(c.id)?.count ?? 0 }));
}
