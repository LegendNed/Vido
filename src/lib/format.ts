/** 75.25 → "1:15.25" (or "1:15" without fraction). */
export function formatTime(seconds: number, fraction = 2): string {
  const s = Math.max(0, Number.isFinite(seconds) ? seconds : 0);
  const m = Math.floor(s / 60);
  const rest = s - m * 60;
  const secStr =
    fraction > 0
      ? rest.toFixed(fraction).padStart(3 + fraction, "0")
      : String(Math.floor(rest)).padStart(2, "0");
  const h = Math.floor(m / 60);
  if (h > 0) return `${h}:${String(m % 60).padStart(2, "0")}:${secStr}`;
  return `${m}:${secStr}`;
}

export function formatDuration(seconds: number): string {
  if (seconds < 60) return `${seconds.toFixed(seconds < 10 ? 1 : 0)} s`;
  return formatTime(seconds, 0);
}

export function formatBytes(bytes: number): string {
  if (bytes < 1000) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = bytes / 1000;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v.toFixed(v < 10 ? 2 : v < 100 ? 1 : 0)} ${units[i] ?? "B"}`;
}

export function formatBitrate(kbps: number): string {
  return kbps >= 1000 ? `${(kbps / 1000).toFixed(1)} Mbps` : `${kbps} kbps`;
}

export function formatDate(ms: number): string {
  const d = new Date(ms);
  const today = new Date();
  const sameDay = d.toDateString() === today.toDateString();
  const time = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  if (sameDay) return `Today ${time}`;
  return `${d.toLocaleDateString(undefined, { month: "short", day: "numeric" })} ${time}`;
}

/** Parses "20 MB", "8mb", "1.5 GB", "500 KB", "25" (MB). Mirrors the Rust parser. */
export function parseSizeMb(input: string): number | null {
  const s = input.trim().toLowerCase().replace(",", ".");
  const m = /^([0-9]*\.?[0-9]+)\s*([a-z]*)$/.exec(s);
  if (!m) return null;
  const v = Number(m[1]);
  if (!Number.isFinite(v) || v <= 0) return null;
  switch (m[2]) {
    case "":
    case "m":
    case "mb":
      return v;
    case "k":
    case "kb":
      return v / 1000;
    case "g":
    case "gb":
      return v * 1000;
    case "mib":
      return v * 1.048576;
    case "gib":
      return v * 1073.741824;
    default:
      return null;
  }
}

export function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

export function dbFromLinear(v: number): number {
  return v <= 0.00001 ? -100 : 20 * Math.log10(v);
}
