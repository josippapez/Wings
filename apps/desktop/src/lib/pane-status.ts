import type { Tone } from "./plugins.ts";

/** One figure in a pane's status strip. `percent` (0 to 100) draws a small meter between the label and the value. */
export type StatusItem = { label?: string; value: string; percent?: number; tone?: Tone };
/** What a plugin shows under a pane's terminal: `wings.setPaneStatus`. `open` is the id of one of the plugin's sidebars or views. */
export type PaneStatus = {
  items: StatusItem[];
  rows?: { label: string; value: string; tone?: Tone }[];
  open?: string;
};

export const STATUS_MAX_ITEMS = 4;
export const STATUS_MAX_ROWS = 12;
export const STATUS_LABEL_MAX = 16;
export const STATUS_VALUE_MAX = 32;
const ROW_LABEL_MAX = 40;
const ROW_VALUE_MAX = 200;

const tones: unknown[] = ["neutral", "info", "success", "warning", "danger", "merged"] satisfies Tone[];

const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);

function text(v: unknown, what: string, max: number, required: boolean): string | undefined {
  if (v === undefined || v === null) {
    if (required) throw new Error(`setPaneStatus: ${what} is required`);
    return undefined;
  }
  if (typeof v !== "string" || (required && !v)) throw new Error(`setPaneStatus: ${what} must be ${required ? "a non-empty " : ""}string`);
  if (v.length > max) throw new Error(`setPaneStatus: ${what} is over ${max} characters`);
  return v;
}

function toneOf(v: unknown, what: string): Tone | undefined {
  if (v === undefined || v === null) return undefined;
  if (!tones.includes(v)) throw new Error(`setPaneStatus: ${what} must be one of ${tones.join(", ")}`);
  return v as Tone;
}

/**
 * Plugin data is untrusted. Unlike a badge, which drops what it doesn't know, a status that breaks a limit is
 * refused with the reason, so the plugin's author sees it. `pages` are the plugin's sidebar and view ids.
 */
export function cleanPaneStatus(raw: unknown, pages: string[]): PaneStatus {
  if (!isObject(raw)) throw new Error("setPaneStatus: status must be an object or null");
  if (!Array.isArray(raw.items) || raw.items.length === 0) throw new Error("setPaneStatus: items must be a non-empty array");
  if (raw.items.length > STATUS_MAX_ITEMS) throw new Error(`setPaneStatus: at most ${STATUS_MAX_ITEMS} items`);
  const items = raw.items.map((item, i): StatusItem => {
    if (!isObject(item)) throw new Error(`setPaneStatus: items[${i}] must be an object`);
    const label = text(item.label, `items[${i}].label`, STATUS_LABEL_MAX, false);
    const value = text(item.value, `items[${i}].value`, STATUS_VALUE_MAX, true)!;
    let percent: number | undefined;
    if (item.percent !== undefined && item.percent !== null) {
      if (typeof item.percent !== "number" || !Number.isFinite(item.percent) || item.percent < 0 || item.percent > 100) {
        throw new Error(`setPaneStatus: items[${i}].percent must be a number from 0 to 100`);
      }
      percent = item.percent;
    }
    return { ...(label ? { label } : {}), value, ...(percent !== undefined ? { percent } : {}), ...(item.tone != null ? { tone: toneOf(item.tone, `items[${i}].tone`) } : {}) };
  });
  let rows: PaneStatus["rows"];
  if (raw.rows !== undefined && raw.rows !== null) {
    if (!Array.isArray(raw.rows)) throw new Error("setPaneStatus: rows must be an array");
    if (raw.rows.length > STATUS_MAX_ROWS) throw new Error(`setPaneStatus: at most ${STATUS_MAX_ROWS} rows`);
    rows = raw.rows.map((row, i) => {
      if (!isObject(row)) throw new Error(`setPaneStatus: rows[${i}] must be an object`);
      const label = text(row.label, `rows[${i}].label`, ROW_LABEL_MAX, true)!;
      const value = text(row.value, `rows[${i}].value`, ROW_VALUE_MAX, true)!;
      return { label, value, ...(row.tone != null ? { tone: toneOf(row.tone, `rows[${i}].tone`) } : {}) };
    });
  }
  let open: string | undefined;
  if (raw.open !== undefined && raw.open !== null) {
    if (typeof raw.open !== "string" || !pages.includes(raw.open)) {
      throw new Error(`setPaneStatus: open must be the id of one of the plugin's sidebars or views${pages.length ? ` (${pages.join(", ")})` : ""}`);
    }
    open = raw.open;
  }
  return { items, ...(rows ? { rows } : {}), ...(open ? { open } : {}) };
}
