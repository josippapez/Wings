// The project info data model and every change to it, as pure functions. Storage and the pages live elsewhere: main
// is the only writer, so each change is applied to what storage holds at that moment.

/** Per project. All of the plugin's storage is limited to 1 MB, and a page's edit travels as a broadcast, limited to 64,000 characters of JSON. */
export const LIMITS = {
  facts: 40,
  key: 60,
  value: 500,
  sections: 20,
  title: 80,
  /**
   * One section's text. A page's edit travels as a broadcast of at most 64,000 characters of JSON, and JSON writes a
   * control character as 6 (`\u0001`), so the largest edit (this text, and nothing else big) is under 6 x 8,000 = 48,000.
   */
  body: 8_000,
  /** Facts, titles and texts together. */
  total: 40_000,
} as const;

export type Who = "you" | "agent";

export interface Fact {
  key: string;
  value: string;
  by: Who;
  updatedAt: number;
}

export interface Section {
  id: string;
  title: string;
  /** Markdown. */
  body: string;
  /** Who last wrote it. An agent's `append` to your section leaves this at "you". */
  by: Who;
  updatedAt: number;
}

/** What is stored under `info:<projectId>`. `v` is the shape's version. */
export interface Info {
  v: 1;
  facts: Fact[];
  sections: Section[];
  updatedAt: number;
}

export const NOTES_TITLE = "Notes";

export const emptyInfo = (): Info => ({ v: 1, facts: [], sections: [], updatedAt: 0 });

const newId = () => Math.random().toString(36).slice(2, 10);

/** The pre-0.2 free-form notes as sections: "Notes", and "Notes (2)"... when the text is longer than one section holds. */
export function fromLegacy(text: string, now: number): Info {
  const info = emptyInfo();
  let rest = text.trim();
  for (let n = 1; rest; n++) {
    let cut = rest.length;
    if (cut > LIMITS.body) {
      const para = rest.lastIndexOf("\n\n", LIMITS.body);
      cut = para > LIMITS.body / 2 ? para : LIMITS.body;
    }
    info.sections.push({ id: newId(), title: n === 1 ? NOTES_TITLE : `${NOTES_TITLE} (${n})`, body: rest.slice(0, cut).trim(), by: "you", updatedAt: now });
    rest = rest.slice(cut).trim();
  }
  info.updatedAt = now;
  return info;
}

/** A stored value as an `Info`, or null when it isn't one. */
export function parseInfo(raw: unknown): Info | null {
  const r = raw as Partial<Info> | null;
  if (!r || r.v !== 1 || !Array.isArray(r.facts) || !Array.isArray(r.sections)) return null;
  const who = (v: unknown): Who => (v === "agent" ? "agent" : "you");
  const num = (v: unknown) => (typeof v === "number" ? v : 0);
  return {
    v: 1,
    facts: r.facts.filter((f) => typeof f?.key === "string" && typeof f.value === "string").map((f) => ({ key: f.key, value: f.value, by: who(f.by), updatedAt: num(f.updatedAt) })),
    sections: r.sections
      .filter((s) => typeof s?.id === "string" && typeof s.title === "string" && typeof s.body === "string")
      .map((s) => ({ id: s.id, title: s.title, body: s.body, by: who(s.by), updatedAt: num(s.updatedAt) })),
    updatedAt: num(r.updatedAt),
  };
}

/** A cheap fingerprint (FNV-1a) of a text. A page sends this and the length of the text it started from instead of the text itself, to keep its edit small. */
export function fingerprint(text: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193);
  return h >>> 0;
}

/** What a page started from: how long its text was and its fingerprint. */
export interface Base {
  length: number;
  hash: number;
}

export const baseOf = (text: string): Base => ({ length: text.length, hash: fingerprint(text) });

/** Characters counted against `LIMITS.total`. */
export const sizeOf = (info: Info) =>
  info.facts.reduce((n, f) => n + f.key.length + f.value.length, 0) + info.sections.reduce((n, s) => n + s.title.length + s.body.length, 0);

export type Op =
  | { kind: "setFact"; key: string; value: string; /** The key being renamed, when a page edits a row's key. */ oldKey?: string }
  | { kind: "removeFact"; key: string }
  /** An agent replaces a section, or creates it. Refused for a section you last wrote. */
  | { kind: "writeSection"; title: string; body: string }
  | { kind: "appendSection"; title: string; body: string }
  /** You edit a section. `base` describes its text when your page last looked: what an agent appended since stays after yours. */
  | { kind: "editSection"; id: string; title: string; body: string; base: Base }
  | { kind: "addSection"; title: string }
  | { kind: "deleteSection"; id: string }
  | { kind: "moveSection"; id: string; dir: -1 | 1 };

const fold = (s: string) => s.trim().toLowerCase();
const oneLine = (s: string) => s.replace(/\s+/g, " ").trim();
const plural = (n: number, word: string) => `${n.toLocaleString("en")} ${word}`;

function cleanTitle(title: unknown): string {
  const t = typeof title === "string" ? oneLine(title) : "";
  if (!t) throw new Error("A title is required.");
  if (t.length > LIMITS.title) throw new Error(`A title is limited to ${plural(LIMITS.title, "characters")}.`);
  return t;
}

function cleanBody(body: unknown, allowEmpty = false): string {
  if (typeof body !== "string" || (!allowEmpty && !body.trim())) throw new Error("markdown is required.");
  const text = body.replace(/\r\n?/g, "\n");
  if (text.length > LIMITS.body) throw new Error(`A section is limited to ${plural(LIMITS.body, "characters")}; this one is ${text.length.toLocaleString("en")}. Split it into two sections.`);
  return text;
}

/**
 * `info` with `op` applied, as `by` ("you" for a page, "agent" for a tool). Throws a message the caller can show.
 * Never changes `info` itself.
 */
export function applyOp(info: Info, op: Op, by: Who, now: number, id: () => string = newId): Info {
  const facts = info.facts.map((f) => ({ ...f }));
  const sections = info.sections.map((s) => ({ ...s }));
  const find = (title: string) => sections.find((s) => fold(s.title) === fold(title));

  switch (op.kind) {
    case "setFact": {
      const key = typeof op.key === "string" ? oneLine(op.key) : "";
      const value = typeof op.value === "string" ? oneLine(op.value) : "";
      if (!key) throw new Error("key is required.");
      if (key.length > LIMITS.key) throw new Error(`A fact's key is limited to ${plural(LIMITS.key, "characters")}.`);
      if (!value) throw new Error("value is required. Use remove_fact to delete a fact.");
      if (value.length > LIMITS.value) throw new Error(`A fact's value is limited to ${plural(LIMITS.value, "characters")}. Put longer text in a section.`);
      const old = op.oldKey === undefined ? undefined : facts.findIndex((f) => fold(f.key) === fold(op.oldKey!));
      const same = facts.findIndex((f) => fold(f.key) === fold(key));
      const at = old !== undefined && old >= 0 ? old : same;
      if (old !== undefined && old >= 0 && same >= 0 && same !== old) throw new Error(`There is already a fact called ${facts[same].key}.`);
      if (at >= 0) {
        if (facts[at].key === key && facts[at].value === value) return info;
        facts[at] = { key, value, by, updatedAt: now };
      } else {
        if (facts.length >= LIMITS.facts) throw new Error(`A project is limited to ${plural(LIMITS.facts, "facts")}. Remove one first.`);
        facts.push({ key, value, by, updatedAt: now });
      }
      break;
    }
    case "removeFact": {
      const at = facts.findIndex((f) => fold(f.key) === fold(String(op.key)));
      if (at < 0) throw new Error(`No fact called ${String(op.key)}.`);
      facts.splice(at, 1);
      break;
    }
    case "writeSection": {
      const title = cleanTitle(op.title);
      const body = cleanBody(op.body);
      const found = find(title);
      if (found) {
        if (found.by === "you") throw new Error(`The section ${found.title} was last written by the user, so write_section won't replace it. Use append to add to it, or pick another title.`);
        if (found.body === body) return info;
        Object.assign(found, { body, by, updatedAt: now });
      } else {
        if (sections.length >= LIMITS.sections) throw new Error(`A project is limited to ${plural(LIMITS.sections, "sections")}. Append to an existing one.`);
        sections.push({ id: id(), title, body, by, updatedAt: now });
      }
      break;
    }
    case "appendSection": {
      const title = cleanTitle(op.title);
      const body = cleanBody(op.body).trim();
      const found = find(title);
      if (found) {
        const joined = found.body.trim() ? `${found.body.replace(/\s+$/, "")}\n\n${body}` : body;
        if (joined.length > LIMITS.body) throw new Error(`The section ${found.title} would pass ${plural(LIMITS.body, "characters")} (it has ${found.body.length.toLocaleString("en")}). Start a new section with a different title.`);
        // `by` stays: an agent's addition doesn't make your section the agent's to replace.
        Object.assign(found, { body: joined, updatedAt: now });
      } else {
        if (sections.length >= LIMITS.sections) throw new Error(`A project is limited to ${plural(LIMITS.sections, "sections")}. Append to an existing one.`);
        sections.push({ id: id(), title, body, by, updatedAt: now });
      }
      break;
    }
    case "editSection": {
      const found = sections.find((s) => s.id === op.id);
      if (!found) throw new Error("That section was removed.");
      const title = cleanTitle(op.title);
      const other = find(title);
      if (other && other !== found) throw new Error(`There is already a section called ${other.title}.`);
      let body = cleanBody(op.body, true);
      // An agent appended since the page read the section: keep it after what you wrote.
      const base = op.base;
      const extended = found.body.length > base.length && fingerprint(found.body.slice(0, base.length)) === base.hash;
      if (extended) body += found.body.slice(base.length);
      if (body.length > LIMITS.body) throw new Error(`With what an agent added meanwhile, the section would pass ${plural(LIMITS.body, "characters")}. Shorten it, or move some to another section.`);
      if (found.title === title && found.body === body) return info;
      Object.assign(found, { title, body, by: "you", updatedAt: now });
      break;
    }
    case "addSection": {
      const title = cleanTitle(op.title);
      if (find(title)) throw new Error(`There is already a section called ${find(title)!.title}.`);
      if (sections.length >= LIMITS.sections) throw new Error(`A project is limited to ${plural(LIMITS.sections, "sections")}.`);
      sections.push({ id: id(), title, body: "", by: "you", updatedAt: now });
      break;
    }
    case "deleteSection": {
      const at = sections.findIndex((s) => s.id === op.id);
      if (at < 0) return info;
      sections.splice(at, 1);
      break;
    }
    case "moveSection": {
      if (op.dir !== 1 && op.dir !== -1) throw new Error("dir is 1 or -1.");
      const at = sections.findIndex((s) => s.id === op.id);
      const to = at + op.dir;
      if (at < 0 || to < 0 || to >= sections.length) return info;
      [sections[at], sections[to]] = [sections[to], sections[at]];
      break;
    }
    default:
      throw new Error("Unknown change.");
  }

  const next: Info = { v: 1, facts, sections, updatedAt: now };
  // Deleting always fits; a change that grows past the total is refused.
  if (sizeOf(next) > LIMITS.total && sizeOf(next) > sizeOf(info)) {
    throw new Error(`A project's info is limited to ${plural(LIMITS.total, "characters")} in all (it has ${sizeOf(info).toLocaleString("en")}). Trim a section or remove facts first.`);
  }
  return next;
}

/** All of a project's info as Markdown, for `read`. */
export function renderInfo(info: Info, name: string): string {
  if (!info.facts.length && !info.sections.length) return `No project info yet for ${name}.`;
  const out = [`# Project info: ${name}`];
  if (info.facts.length) out.push("## Facts", info.facts.map((f) => `- ${f.key}: ${f.value}`).join("\n"));
  for (const s of info.sections) out.push(`## ${s.title}\n_Last written by ${s.by === "you" ? "the user" : "an agent"}._\n\n${s.body.trim() || "(empty)"}`);
  return out.join("\n\n");
}
