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
