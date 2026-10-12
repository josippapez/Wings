const MINUTE = 60_000;

export const text = (v: unknown, max = 120) => String(v ?? "").replace(/\s+/g, " ").trim().slice(0, max);
export const tokens = (n: number) => (n >= 1e6 ? `${+(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${Math.round(n / 1e3)}k` : String(n));
export const clock = (ms: number) => new Date(ms).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
/** `6.6 GB`, `16 GB`, rounded as claude-hud does. */
export const gb = (bytes: number) => `${(bytes / 2 ** 30).toFixed(bytes >= 10 * 2 ** 30 ? 0 : 1)} GB`;
export const names = (all: string[], max = 4) => (all.length > max ? `${all.slice(0, max).join(", ")}, +${all.length - max} more` : all.join(", "));

/** `2h 54m`, `2d 11h`, `5m`; `compact` drops the spaces and a zero remainder for the title bar: `2h54m`, `2d11h`, `3d`. */
export function until(epochSeconds: number, now = Date.now(), compact = false) {
  let minutes = Math.max(0, Math.round((epochSeconds * 1000 - now) / MINUTE));
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  minutes %= 60;
  const gap = compact ? "" : " ";
  if (days) return hours || !compact ? `${days}d${gap}${hours}h` : `${days}d`;
  return hours ? `${hours}h${gap}${minutes}m` : `${minutes}m`;
}
