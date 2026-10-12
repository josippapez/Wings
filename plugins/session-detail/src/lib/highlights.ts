// ---------- highlights: what in a command is worth seeing at a glance ----------

import { oneLine } from "./format";
import type { Tag } from "./types";

// Each rule matches the start of one command in a shell line, so `rg 'git commit'` isn't a commit.
const GIT = String.raw`(?:\S*/)?git(?:\s+-C\s+\S+)?\s+`;
const RULES: { kind: string; label: string; re: RegExp }[] = [
  { kind: "commit", label: "Commit", re: new RegExp(`^${GIT}commit\\b`) },
  { kind: "push", label: "Push", re: new RegExp(`^${GIT}push\\b`) },
  { kind: "merge", label: "Merge", re: new RegExp(`^${GIT}(?:merge|rebase)\\s+(?!--abort)`) },
  { kind: "branch", label: "Branch", re: new RegExp(`^${GIT}(?:checkout\\s+-b|switch\\s+-c|worktree\\s+add)\\s`) },
  { kind: "pr", label: "Pull request", re: /^gh\s+pr\s+(?:create|merge)\b/ },
  { kind: "test", label: "Tests", re: /^(?:cargo\s+test|(?:pnpm|npm|yarn|bun)\s+(?:run\s+)?test|(?:npx\s+)?(?:vitest|jest)|pytest|go\s+test)\b/ },
  { kind: "build", label: "Build", re: /^(?:cargo\s+build|(?:pnpm|npm|yarn|bun)\s+(?:run\s+|exec\s+)?(?:tauri\s+)?build|vite\s+build)\b/ },
  { kind: "install", label: "Install", re: /^(?:(?:pnpm|npm|yarn|bun)\s+(?:install|add|i)|cargo\s+add|brew\s+install)\b/ },
  // Easy to regret, so they stand out.
  { kind: "danger", label: "Force push", re: new RegExp(`^${GIT}push\\b.*\\s(?:--force|-f)\\b`) },
  { kind: "danger", label: "Reset", re: new RegExp(`^${GIT}reset\\s+--hard\\b`) },
  { kind: "danger", label: "Delete", re: /^rm\s+-(?:[a-z]*r[a-z]*f|[a-z]*f[a-z]*r)\b/ },
];

/** The commands in a shell line, split at `&&`, `||`, `|`, `;` and newlines outside quotes, without leading
 * `VAR=value` assignments or heredoc bodies, which are data. Enough for the lines Claude writes, not a full
 * shell parser. */
function commands(line: string) {
  const out: string[] = [];
  let current = "";
  let quote: string | null = null;
  let heredoc: string | null = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (!quote && c === "<" && line[i + 1] === "<" && line[i + 2] !== "<") {
      heredoc = /^<<-?\s*['"]?([A-Za-z_][A-Za-z0-9_]*)/.exec(line.slice(i))?.[1] ?? heredoc;
    }
    if (!quote && c === "\n" && heredoc) {
      // Skip to the line that ends the heredoc.
      const close = line.slice(i + 1).search(new RegExp(`^\\s*${heredoc}\\s*$`, "m"));
      i = close < 0 ? line.length : line.indexOf("\n", i + 1 + close);
      if (i < 0) i = line.length;
      heredoc = null;
    }
    if (quote) {
      current += c;
      if (c === "\\" && quote === '"') current += line[++i] ?? "";
      else if (c === quote) quote = null;
    } else if (c === "'" || c === '"') {
      quote = c;
      current += c;
    } else if ("&|;\n".includes(c)) {
      if (current.trim()) out.push(current.trim());
      current = "";
    } else current += c;
  }
  if (current.trim()) out.push(current.trim());
  return out.map((c) => c.replace(/^(?:[A-Za-z_][A-Za-z0-9_]*=\S*\s+)+/, ""));
}

/** The commit message's first line, from `-m "..."`, `-m '...'` or a `$(cat <<'EOF'` heredoc. */
function commitMessage(command: string) {
  const m = /\s-m\s+(?:"((?:[^"\\]|\\.)*)"|'([^']*)')/.exec(command);
  if (!m) return null;
  let text = (m[1] ?? m[2]).replace(/\\(.)/g, "$1");
  if (/^\$\(cat\s+<</.test(text)) text = text.split("\n").slice(1).join("\n");
  return oneLine(text.split("\n").find((l) => l.trim()) ?? "", 100) || null;
}

/** Labels for the notable things a shell line does, and a title to show instead of the raw line. */
export function highlights(line: unknown) {
  const tags: Tag[] = [];
  let title: string | null = null;
  for (const command of commands(String(line ?? ""))) {
    for (const rule of RULES) {
      if (!rule.re.test(command) || tags.some((t) => t.label === rule.label)) continue;
      tags.push({ kind: rule.kind, label: rule.label });
      if (rule.kind === "commit") title ??= commitMessage(command);
    }
  }
  return tags.length ? { tags, title } : null;
}

/** Pass and fail counts from test output, for the Tests label. */
export function testOutcome(output: string) {
  let passed = 0;
  let failed = 0;
  for (const m of output.matchAll(/test result: \w+\. (\d+) passed; (\d+) failed/g)) (passed += +m[1]), (failed += +m[2]);
  for (const m of output.matchAll(/^\D{0,4}(pass|fail) (\d+)$/gm)) m[1] === "pass" ? (passed += +m[2]) : (failed += +m[2]);
  for (const m of output.matchAll(/Tests:?\s+(?:(\d+) failed, )?(\d+) passed/g)) (failed += +(m[1] ?? 0)), (passed += +m[2]);
  return passed || failed ? (failed ? `${failed} failed` : `${passed} passed`) : null;
}
