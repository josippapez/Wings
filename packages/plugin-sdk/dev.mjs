// wings-plugin dev: builds the plugin when its files change, then tells the running Wings to read the folder
// again (the link op), which restarts the plugin's frames. A failed build prints its error and keeps watching.
import { readFileSync, watch as fsWatch } from "node:fs";
import { join, relative, sep } from "node:path";
import { buildPlugin } from "./build.mjs";
import { link as linkWings } from "./socket.mjs";

/** The build writes dist/, and these hold files nobody edits by hand. */
const IGNORED = new Set(["dist", "node_modules", ".git"]);

const defaultWatch = (dir, onChange) => {
  const watcher = fsWatch(dir, { recursive: true }, (_event, name) => onChange(name));
  return { close: () => watcher.close() };
};

const readManifest = (dir) => {
  try {
    return readFileSync(join(dir, "wings-plugin.json"), "utf8");
  } catch {
    return null;
  }
};

/**
 * Watches `dir`. `watch(dir, onChange)` and `build(dir)` can be replaced, which the tests do. Resolves once the
 * first build and link attempt have run, with `close()`.
 */
export async function dev(dir, { watch = defaultWatch, build = buildPlugin, link = linkWings, log = console.log, error = console.error, debounce = 150, retry = 2000 } = {}) {
  let timer;
  let retryTimer;
  let building = false;
  let again = false;
  let closed = false;
  let lastManifest = null;

  const tryLink = async () => {
    clearTimeout(retryTimer);
    try {
      const plugin = await link(dir);
      if (plugin && plugin.approved === false) log("Linked. Wings is waiting for you to approve the plugin: open the Plugins sheet and review its access.");
      else log("Linked: Wings restarted the plugin.");
    } catch (e) {
      if (e?.kind === "notRunning") {
        error(`Wings isn't running. Open Wings and this will link the plugin, trying again every ${retry / 1000} s.`);
        if (!closed) retryTimer = setTimeout(tryLink, retry);
      } else if (e?.kind === "unknownOp") {
        error("This Wings is too old to link a plugin folder. Update Wings, then save a file here to try again.");
      } else {
        error(`Wings didn't take the plugin: ${e?.message ?? e}`);
      }
    }
  };

  const run = async () => {
    if (building) {
      again = true;
      return;
    }
    building = true;
    try {
      do {
        again = false;
        try {
          const { built } = await build(dir);
          lastManifest = readManifest(dir);
          log(`Built ${built.length} file${built.length === 1 ? "" : "s"}.`);
          await tryLink();
        } catch (e) {
          lastManifest = readManifest(dir);
          error(e?.message ?? String(e));
        }
      } while (again && !closed);
    } finally {
      building = false;
    }
  };

  const changed = (name) => {
    if (closed) return;
    const file = name == null ? "" : relative(dir, join(dir, String(name)));
    if (file && IGNORED.has(file.split(sep)[0])) return;
    // The build rewrites the manifest from src/tools.ts. Its own write isn't a change to build for.
    if (file === "wings-plugin.json" && readManifest(dir) === lastManifest) return;
    clearTimeout(timer);
    timer = setTimeout(run, debounce);
  };

  const watcher = watch(dir, changed);
  log(`Watching ${dir}`);
  await run();
  return {
    close() {
      closed = true;
      clearTimeout(timer);
      clearTimeout(retryTimer);
      watcher.close();
    },
  };
}
