// A made-up scan of a startup disk for the fixtures: the same tree every run, with folders, files, smaller objects
// and restricted folders, and sizes that add up the way a real scan's do.

import type { Entry } from "../src/lib/messages";

const GB = 1e9;
const MB = 1e6;

/** A file of `size` bytes. */
const file = (name: string, size: number): Entry => ({ name, size, kind: "file", items: 0 });

/** A folder holding `children`, plus `smaller` bytes in `smallItems` entries too small to keep. */
function dir(name: string, children: Entry[], smaller = 0, smallItems = 0): Entry {
  const sorted = [...children].sort((a, b) => b.size - a.size);
  const size = sorted.reduce((sum, c) => sum + c.size, smaller);
  const items = sorted.reduce((sum, c) => sum + c.items + 1, smallItems);
  return { name, size, kind: "dir", items, children: sorted, ...(smaller > 0 ? { smaller: { size: smaller, items: smallItems } } : {}) };
}

const restricted = (name: string): Entry => ({ name, size: 0, kind: "dir", items: 0, restricted: true });

/** A folder of `count` made-up projects of falling size, like a code folder. */
const projects = (count: number, first: number) =>
  Array.from({ length: count }, (_, i) =>
    dir(`project-${String.fromCharCode(97 + i)}`, [dir("node_modules", [], first / (i + 2), 4000 + i * 300), dir("src", [], first / (i + 2) / 20, 300), file("bundle.zip", first / (i + 3))]),
  );

const home = dir(
  "ada",
  [
    dir(
      "Library",
      [
        dir("Developer", [dir("Xcode", [dir("DerivedData", [], 38 * GB, 90000), dir("iOS DeviceSupport", [], 21 * GB, 4000)]), dir("CoreSimulator", [], 26 * GB, 120000)]),
        dir("Caches", [dir("com.spotify.client", [], 6.2 * GB, 1200), dir("pnpm", [], 9.8 * GB, 300000), dir("Homebrew", [], 3.1 * GB, 400)], 1.4 * GB, 9000),
        dir("Application Support", [dir("Code", [], 4.4 * GB, 20000), dir("Slack", [], 2.6 * GB, 8000), restricted("com.apple.TCC")], 3.3 * GB, 30000),
        restricted("Mail"),
        dir("Containers", [], 12 * GB, 60000),
      ],
      2.2 * GB,
      40000,
    ),
    dir("Movies", [file("Holiday 2025.mov", 18.4 * GB), file("Screen Recording.mov", 4.9 * GB), dir("Final Cut", [], 7.5 * GB, 600)]),
    dir("Code", projects(7, 16 * GB), 1.1 * GB, 9000),
    dir("Downloads", [file("ubuntu-26.04-desktop-arm64.iso", 6.1 * GB), file("Xcode_27.xip", 11.2 * GB), dir("old installers", [], 3.4 * GB, 40)], 2.6 * GB, 800),
    dir("Pictures", [dir("Photos Library.photoslibrary", [], 41 * GB, 70000)], 0.6 * GB, 300),
    dir("Music", [], 8.8 * GB, 5200),
    dir("Documents", [dir("Taxes", [], 0.4 * GB, 200), file("archive.tar.gz", 5.6 * GB)], 3.9 * GB, 6000),
  ],
  1.8 * GB,
  2000,
);

/** The root of a scan of `/`, the startup disk. */
export const tree: Entry = {
  ...dir(
    "/",
    [
      dir("Users", [home, dir("Shared", [], 1.2 * GB, 300), restricted("guest")]),
      dir("Applications", [dir("Xcode.app", [], 12.8 * GB, 140000), dir("Docker.app", [], 2.1 * GB, 9000), dir("Wings.app", [], 48 * MB, 300)], 9.4 * GB, 60000),
      dir("System", [dir("Library", [], 11.6 * GB, 300000)], 1.3 * GB, 40000),
      dir("Library", [dir("Developer", [], 14 * GB, 50000), dir("Application Support", [], 3.2 * GB, 9000)], 4.4 * GB, 30000),
      dir("private", [dir("var", [restricted("db"), dir("folders", [], 4.1 * GB, 30000)], 2.4 * GB, 9000)]),
      dir("opt", [dir("homebrew", [], 7.7 * GB, 120000)]),
    ],
    0.8 * GB,
    3000,
  ),
};

/** Where the fixtures' scan is rooted: the root entry's name. */
export const root = tree.name;
/** The fixtures' home folder. */
export const homePath = "/Users/ada";
