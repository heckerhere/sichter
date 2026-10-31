import { intlTag, locale, t } from "./i18n";

// Formatierer je Sprache, bei Bedarf neu angelegt.
const cache = new Map<string, { bytes: Intl.NumberFormat; int: Intl.NumberFormat; regions: Intl.DisplayNames | null }>();
function fmt() {
  const tag = intlTag();
  let f = cache.get(tag);
  if (!f) {
    let regions: Intl.DisplayNames | null = null;
    try {
      regions = new Intl.DisplayNames([tag], { type: "region" });
    } catch {
      /* ältere WebViews */
    }
    f = { bytes: new Intl.NumberFormat(tag, { maximumFractionDigits: 1 }), int: new Intl.NumberFormat(tag), regions };
    cache.set(tag, f);
  }
  return f;
}
const en = () => locale.value === "en";

export function formatBytes(n: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = n;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  return `${unit === 0 ? value : fmt().bytes.format(value)} ${units[unit]}`;
}

export const formatInt = (n: number) => fmt().int.format(n);

/** Kalenderdatum: "12.05.2023" bzw. englisch "2023-05-12" (eindeutig in allen Ländern). */
function day(y: string, m: string, d: string): string {
  return en() ? `${y}-${m}-${d}` : `${d}.${m}.${y}`;
}

/** "2023-05-12T15:01:10" -> "12.05.2023 15:01" bzw. "2023-05-12 15:01" */
export function formatDate(iso: string | null): string {
  if (!iso) return t.common.noDate;
  const [date, time = ""] = iso.split("T");
  const [y, m, d] = date.split("-");
  return `${day(y, m, d)}${time ? " " + time.slice(0, 5) : ""}`;
}

export function formatDuration(s: number): string {
  const total = Math.round(s);
  const m = Math.floor(total / 60);
  return `${m}:${String(total % 60).padStart(2, "0")}`;
}

export const fileName = (relPath: string) => relPath.slice(relPath.lastIndexOf("/") + 1);

/** "HR" -> "Kroatien" / "Croatia" (fällt auf den Code zurück). */
export const countryName = (code: string) => fmt().regions?.of(code) ?? code;

export const monthName = (m: number) => t.common.months[m - 1] ?? "";

/** Zeitraum eines Events: "12.05.2023, 15:00–17:30" oder "12.–15.07.2023"
 *  (englisch: "2023-05-12, 15:00–17:30" bzw. "2023-07-12 – 07-15"). */
export function formatRange(start: string | null, end: string | null): string {
  if (!start) return t.common.noDate;
  const [sd, st = ""] = start.split("T");
  const [ed, et = ""] = (end ?? start).split("T");
  const [sy, sm, sday] = sd.split("-");
  const [ey, em, eday] = ed.split("-");
  if (sd === ed) {
    const times = st.slice(0, 5) === et.slice(0, 5) ? st.slice(0, 5) : `${st.slice(0, 5)}–${et.slice(0, 5)}`;
    return `${day(sy, sm, sday)}, ${times}`;
  }
  if (en()) {
    if (sy === ey) return `${sy}-${sm}-${sday} – ${em}-${eday}`;
    return `${sd} – ${ed}`;
  }
  if (sy === ey && sm === em) return `${sday}.–${eday}.${em}.${ey}`;
  if (sy === ey) return `${sday}.${sm}.–${eday}.${em}.${ey}`;
  return `${sday}.${sm}.${sy}–${eday}.${em}.${ey}`;
}

/** Anzahl Kalendertage eines Zeitraums (1 = ein Tag). */
export function dayCount(start: string | null, end: string | null): number {
  if (!start || !end) return 1;
  const d = (s: string) => Date.UTC(+s.slice(0, 4), +s.slice(5, 7) - 1, +s.slice(8, 10));
  return Math.round((d(end) - d(start)) / 86_400_000) + 1;
}
