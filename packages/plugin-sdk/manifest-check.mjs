// Finds where a plugin's wings-plugin.json and its built code disagree, which Wings only finds out when it loads the
// plugin or when the code runs. The bundles are searched as text, and the minifier keeps string literals, so a name
// written out in the code is checked and a computed one, like `wings.openView(id)`, is skipped. So is a constant the
// minifier keeps as a variable, like one used more than once.
import { statSync } from "node:fs";
import { join, posix } from "node:path";

/** The `wings` calls that need a `contributes.ui` entry, by that entry. Wings refuses them without it. */
const SURFACES = { badges: ["setBadge"], status: ["setPaneStatus"], diff: ["openDiff", "updateDiff"] };

/** The `wings` calls that take the id of a sidebar or view as their first argument. */
const PAGE_CALLS = ["openView", "closeView", "setSidebarLabel"];

/**
 * An `open` in an object, as the minifier writes it: after `{` or `,`, which leaves out `a.open` and `a ? b.open : "c"`.
 * A pane status has `open: "id"`, and a project section row or a notification button has `open: { view: "id", ... }`.
 */
const OPEN = "[{,]open:";
const OPEN_VIEW = "[{,]open:\\{view:";

/** A manifest path as Wings reads it: from the plugin folder, without a leading `/`. */
const inFolder = (path) => posix.normalize(String(path).replace(/^\/+/, ""));

/** Whether `file`, from `inFolder`, is a file in `root`. */
const hasFile = (root, file) => statSync(join(root, file), { throwIfNoEntry: false })?.isFile() === true;

/** The strings written out in `text` right after `before`, a regular expression. `"a" + b` and a template with `${}` are not. */
const literals = (text, before) => Array.from(text.matchAll(new RegExp(`${before}\\s*(["'\`])([^"'\`\\\\$]*)\\1(?=\\s*[,)}])`, "g")), (m) => m[2]);

/** The names `wings.<method>` is called with in `text`, where the name is a string written out. */
const named = (text, method) => literals(text, `\\bwings\\.${method}\\(`);

/** The manifest's fields that name a page, as `[field, path]`. */
function pagesNamed({ sidebars = [], views = [], blocks = [], settings }) {
  return [
    ...sidebars.flatMap((s, i) => [[`contributes.sidebars[${i}].page`, s.page], [`contributes.sidebars[${i}].popover.page`, s.popover?.page]]),
    ...views.map((v, i) => [`contributes.views[${i}].page`, v.page]),
    ...blocks.map((b, i) => [`contributes.blocks[${i}].page`, b.page]),
    ["contributes.settings.page", settings?.page],
  ].filter(([, path]) => path !== undefined);
}

/**
 * What `manifest`, a parsed wings-plugin.json, says that the build's output doesn't bear out: a sentence for each
 * problem, none when they agree. `root` is the folder Wings loads the plugin from, `built` is the `{ file, text }` the
 * build writes there, and `main` is the file it writes the main script to.
 */
export function manifestProblems(manifest, { root, built, main }) {
  const { contributes = {} } = manifest;
  const problems = [];
  const written = built.map((b) => b.file);
  const bundles = built.filter(({ file }) => file.endsWith(".js"));
  if (inFolder(manifest.main) !== main) problems.push(`main is "${manifest.main}", but the build writes the main script to ${main}.`);
  for (const [field, path] of pagesNamed(contributes)) {
    const file = inFolder(path);
    if (written.includes(file) || hasFile(root, file)) continue;
    const writes = written.filter((f) => f.endsWith(posix.extname(file)));
    problems.push(`${field} is "${path}", which the build doesn't write and the plugin folder doesn't have${writes.length ? `. It writes ${writes.join(", ")}` : ""}.`);
  }
  // The build doesn't write icons, so the file has to be in the folder.
  if (manifest.icon != null && !hasFile(root, inFolder(manifest.icon))) problems.push(`icon is "${manifest.icon}", which the plugin folder doesn't have.`);

  const declared = (contributes.mcpTools ?? []).map((t) => t.name);
  const mainText = built.find((b) => b.file === main).text;
  const answered = named(mainText, "onTool");
  for (const name of answered.filter((n) => !declared.includes(n))) {
    problems.push(`${main} answers "${name}" with wings.onTool, but contributes.mcpTools has no tool of that name${declared.length ? ` (it has ${declared.join(", ")})` : ""}.`);
  }
  // A call that takes the name from a variable, like registerTools makes for each of its tools, answers names this can't read.
  if (mainText.split(".onTool(").length - 1 === answered.length) {
    for (const name of declared.filter((n) => !answered.includes(n))) problems.push(`contributes.mcpTools lists "${name}", but nothing in ${main} answers it with wings.onTool or registerTools.`);
  }

  for (const [kind, methods] of Object.entries(SURFACES)) {
    if ((contributes.ui ?? []).includes(kind)) continue;
    for (const { file, text } of bundles) {
      const method = methods.find((m) => text.includes(`wings.${m}(`));
      if (!method) continue;
      problems.push(`${file} calls wings.${method}, so contributes.ui must list "${kind}".`);
      break;
    }
  }

  const pages = [...(contributes.sidebars ?? []), ...(contributes.views ?? [])].map((page) => page.id);
  const noPage = `contributes.sidebars and contributes.views have no such id (they have ${pages.join(", ") || "none"})`;
  // A project section row and a notification button open a view as a tab, so a sidebar won't do.
  const tabs = (contributes.views ?? []).filter((view) => view.opens?.includes("tab")).map((view) => view.id);
  for (const { file, text } of bundles) {
    for (const method of PAGE_CALLS) {
      for (const id of new Set(named(text, method))) {
        if (!pages.includes(id)) problems.push(`${file} calls wings.${method}("${id}"), but ${noPage}.`);
      }
    }
    // An `open` key can be anything, so it's only read in a bundle that calls what takes it.
    if (text.includes("wings.setPaneStatus(")) {
      for (const id of new Set(literals(text, OPEN))) {
        if (!pages.includes(id)) problems.push(`${file} sets open: "${id}" on a pane status, but ${noPage}.`);
      }
    }
    if (text.includes("wings.setProjectSection(") || text.includes("wings.notify(")) {
      for (const id of new Set(literals(text, OPEN_VIEW))) {
        if (!tabs.includes(id)) problems.push(`${file} sets open: { view: "${id}" } on a project section row or a notification button, but contributes.views has no view of that id with "tab" in opens (the views that have it: ${tabs.join(", ") || "none"}).`);
      }
    }
  }
  return problems;
}
