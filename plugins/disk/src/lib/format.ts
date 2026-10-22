/** Bytes the way Finder writes them, in powers of 1000: `980 KB`, `12.3 GB`, `1 byte`. */
export function bytes(n: number): string {
  if (n < 1000) return n === 1 ? "1 byte" : `${n} bytes`;
  const units = ["KB", "MB", "GB", "TB", "PB"];
  let value = n / 1000;
  let unit = 0;
  while (value >= 999.95 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  return `${value >= 100 ? Math.round(value) : value.toFixed(1).replace(/\.0$/, "")} ${units[unit]}`;
}
