// wings-plugin init: copies the starter plugin into a new folder, names it after the folder and installs its packages.
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, readdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";

const here = import.meta.dirname;

/** The id a folder name makes: lowercase letters, digits and dashes, up to 64, which is what Wings accepts. */
export function idOf(folder) {
  return basename(resolve(folder))
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64)
    .replace(/-+$/, "");
}

/** A readable name from an id: "my-plugin" is "My plugin". */
const nameOf = (id) => {
  const words = id.replaceAll("-", " ");
  return words[0].toUpperCase() + words.slice(1);
};

const readJson = (file) => JSON.parse(readFileSync(file, "utf8"));
const writeJson = (file, value) => writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);

/**
 * Creates the plugin in `folder`, which has to be new or empty. `install` is the package manager to run there, or
 * false to skip it. `run(command, args, cwd)` can be replaced, which the tests do; it returns the exit code.
 */
export function init(folder, { install = "npm", run = defaultRun, log = console.log } = {}) {
  const dir = resolve(folder);
  const id = idOf(dir);
  if (!id) throw new Error(`Can't make a plugin id from the folder name "${basename(dir)}": use letters or digits.`);
  if (existsSync(dir) && readdirSync(dir).length) throw new Error(`${dir} already has files. Run wings-plugin init in a new or empty folder.`);
  cpSync(join(here, "template"), dir, { recursive: true });
  // npm leaves a .gitignore out of a package, so the template keeps it as `gitignore`.
  renameSync(join(dir, "gitignore"), join(dir, ".gitignore"));

  const manifestFile = join(dir, "wings-plugin.json");
  const manifest = readJson(manifestFile);
  const starter = manifest.id;
  const name = nameOf(id);
  writeJson(manifestFile, { ...manifest, id, name });

  const pkgFile = join(dir, "package.json");
  const pkg = readJson(pkgFile);
  pkg.devDependencies["wings-plugin-sdk"] = `^${readJson(join(here, "package.json")).version}`;
  pkg.scripts = { dev: "wings-plugin dev", build: "wings-plugin build", pack: "wings-plugin pack", check: "tsc && wings-plugin pack" };
  writeJson(pkgFile, pkg);

  const readme = join(dir, "README.md");
  writeFileSync(readme, readFileSync(readme, "utf8").replaceAll(starter, id).replaceAll("Hello Wings", name));
  log(`Created ${name} (${id}) in ${dir}`);

  if (install) {
    log(`Running ${install} install...`);
    if (run(install, ["install"], dir) !== 0) throw new Error(`${install} install failed. The plugin is in ${dir}: fix that and run it again there.`);
  }
  log(`Next: ${install ? "" : "install its packages, then "}run ${install || "npm"} run dev in ${dir} to build on each save and have Wings restart the plugin.`);
  return { dir, id, name };
}

function defaultRun(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, stdio: "inherit", shell: process.platform === "win32" });
  return result.status ?? 1;
}
