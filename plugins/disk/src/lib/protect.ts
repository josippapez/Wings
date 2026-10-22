// What the Collector refuses, so a click can't queue a whole disk, a system folder or a home folder for the Trash. The
// folders that are only refused themselves have contents that may be collected: `/Library/Caches` is fine, `/Library` isn't.

/** These folders, and everything inside them. */
const SYSTEM = ["/system", "/bin", "/sbin", "/usr", "/private/var/db", "/library/apple"];
/** Inside `/usr`, this one is for the user's own programs. */
const USER_PROGRAMS = "/usr/local/";
/** These folders themselves. `/var`, `/etc` and `/tmp` are `/private/...`: see `canonical`. */
const FOLDERS = ["/applications", "/library", "/users", "/private", "/private/var", "/private/etc", "/private/tmp", "/opt", "/cores", "/volumes", "/dev", "/home"];
const HOME = /^\/(users|home)\/[^/]+$/;
const HOME_FOLDER = /^\/(users|home)\/[^/]+\/(library|desktop|documents|downloads|applications|movies|music|pictures|public)$/;

/** `path` with `.`, `..`, repeated and trailing slashes resolved, in lower case: macOS volumes ignore case. */
function simplify(path: string): string {
  const parts: string[] = [];
  for (const part of path.toLowerCase().split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") parts.pop();
    else parts.push(part);
  }
  return `/${parts.join("/")}`;
}

/** `/var`, `/etc` and `/tmp` are links into `/private`, and a path through them is the same folder. */
const canonical = (path: string) => (/^\/(var|etc|tmp)(\/|$)/.test(path) ? `/private${path}` : path);

const inside = (path: string, folder: string) => path === folder || path.startsWith(`${folder}/`);

/**
 * Why `path` may not go in the Collector, as a sentence that starts with it, or null when it may. `mounts` are the mount
 * points of the disks, which are refused like `/`. A path that isn't absolute is refused too.
 */
export function protectedReason(path: string, mounts: string[]): string | null {
  if (!path.startsWith("/")) return `${path} isn't a full path.`;
  const p = canonical(simplify(path));
  if (p === "/" || mounts.some((m) => canonical(simplify(m)) === p)) return `${path} is a whole disk.`;
  if (SYSTEM.some((folder) => inside(p, folder)) && !p.startsWith(USER_PROGRAMS)) return `${path} is a system folder.`;
  if (FOLDERS.includes(p)) return `${path} is a system folder.`;
  if (HOME.test(p)) return `${path} is a home folder.`;
  if (HOME_FOLDER.test(p)) return `${path} is one of the standard folders of a home folder.`;
  return null;
}
