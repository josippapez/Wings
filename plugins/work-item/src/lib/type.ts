// How a work item type looks: Azure DevOps' own colours for the common ones, and which glyph the icon draws.

export type Glyph = "bug" | "task" | "story" | "feature" | "epic" | "issue" | "test" | "other";

const TYPES: { match: RegExp; glyph: Glyph; color: string }[] = [
  { match: /bug/i, glyph: "bug", color: "#cc293d" },
  { match: /^task$/i, glyph: "task", color: "#f2cb1d" },
  { match: /user story|product backlog item|requirement/i, glyph: "story", color: "#009ccc" },
  { match: /feature/i, glyph: "feature", color: "#773b93" },
  { match: /epic/i, glyph: "epic", color: "#ff7b00" },
  { match: /issue|impediment|risk/i, glyph: "issue", color: "#b4009e" },
  { match: /test/i, glyph: "test", color: "#004b50" },
];

/** The glyph and colour of a work item type; a type we don't know is a grey dot. */
export function typeStyle(type: string): { glyph: Glyph; color: string } {
  const t = TYPES.find((x) => x.match.test(type));
  return t ? { glyph: t.glyph, color: t.color } : { glyph: "other", color: "#8a8a8a" };
}
