const bytesFmt = new Intl.NumberFormat("de-DE", { maximumFractionDigits: 1 });
const intFmt = new Intl.NumberFormat("de-DE");

export function formatBytes(n: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = n;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  return `${unit === 0 ? value : bytesFmt.format(value)} ${units[unit]}`;
}

export const formatInt = (n: number) => intFmt.format(n);

/** "2023-05-12T15:01:10" -> "12.05.2023 15:01" */
export function formatDate(iso: string | null): string {
  if (!iso) return "ohne Datum";
  const [date, time = ""] = iso.split("T");
  const [y, m, d] = date.split("-");
  return `${d}.${m}.${y}${time ? " " + time.slice(0, 5) : ""}`;
}

export function formatDuration(s: number): string {
  const total = Math.round(s);
  const m = Math.floor(total / 60);
  return `${m}:${String(total % 60).padStart(2, "0")}`;
}

export const fileName = (relPath: string) => relPath.slice(relPath.lastIndexOf("/") + 1);

const MONTHS = ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"];
export const monthName = (m: number) => MONTHS[m - 1] ?? "";

/** Zeitraum eines Events: "12.05.2023, 15:00–17:30" oder "12.–15.07.2023". */
export function formatRange(start: string | null, end: string | null): string {
  if (!start) return "ohne Datum";
  const [sd, st = ""] = start.split("T");
  const [ed, et = ""] = (end ?? start).split("T");
  const [sy, sm, sday] = sd.split("-");
  const [ey, em, eday] = ed.split("-");
  if (sd === ed) {
    const times = st.slice(0, 5) === et.slice(0, 5) ? st.slice(0, 5) : `${st.slice(0, 5)}–${et.slice(0, 5)}`;
    return `${sday}.${sm}.${sy}, ${times}`;
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
