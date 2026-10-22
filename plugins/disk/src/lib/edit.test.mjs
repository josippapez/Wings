import test from "node:test";
import assert from "node:assert/strict";
import { graft, removePath } from "./edit.ts";

const file = (name, size) => ({ name, size, kind: "file", items: 0 });
/** A folder of `children` and `smaller` bytes in `smallItems` entries too small to keep, sized the way a scan sizes it. */
function dir(name, children, smaller = 0, smallItems = 0) {
  const sorted = [...children].sort((a, b) => b.size - a.size);
  return {
    name,
    kind: "dir",
    size: sorted.reduce((sum, c) => sum + c.size, smaller),
    items: sorted.reduce((sum, c) => sum + c.items + 1, smallItems),
    ...(sorted.length ? { children: sorted } : {}),
    ...(smaller ? { smaller: { size: smaller, items: smallItems } } : {}),
  };
}

/** Every folder's size and items add up from what is in it, and its children are biggest first. */
function assertConsistent(entry) {
  if (entry.kind !== "dir") return;
  const kids = entry.children ?? [];
  assert.equal(entry.size, kids.reduce((sum, c) => sum + c.size, entry.smaller?.size ?? 0), `${entry.name}: size`);
  assert.equal(entry.items, kids.reduce((sum, c) => sum + c.items + 1, entry.smaller?.items ?? 0), `${entry.name}: items`);
  assert.deepEqual(kids.map((c) => c.size), kids.map((c) => c.size).sort((a, b) => b - a), `${entry.name}: order`);
  kids.forEach(assertConsistent);
}

const tree = () =>
  dir("/Users/ada", [
    dir("Library", [dir("Caches", [dir("pnpm", [file("store.db", 700)], 200, 20)], 100, 10), dir("Logs", [], 600, 50)]),
    dir("Movies", [file("holiday.mov", 800)], 0),
  ]);
const at = (t, ...names) => names.reduce((e, n) => e.children.find((c) => c.name === n), t);

test("graft puts a finer scan in and adds the size and items difference to every folder above, keeping each in order", () => {
  const t = tree();
  assertConsistent(t);
  const before = { size: t.size, items: t.items };
  const caches = at(t, "Library", "Caches");
  // Caches shrinks from 1000 to 300 bytes and from 32 to 3 items, which puts it below its sibling Logs.
  const fresh = dir("/Users/ada/Library/Caches", [dir("pnpm", [], 250, 2)], 50, 0);
  assert.equal(graft(t, "/Users/ada/Library/Caches", fresh, 64), true);
  assert.equal(caches.size, 300);
  assert.equal(caches.items, 3);
  assert.equal(caches.detail, 64);
  assert.deepEqual(caches.children, fresh.children);
  assert.deepEqual(caches.smaller, { size: 50, items: 0 });
  assert.equal(caches.name, "Caches");
  assert.equal(t.size, before.size - 700);
  assert.equal(t.items, before.items - 29);
  assertConsistent(t);
  assert.deepEqual(at(t, "Library").children.map((c) => c.name), ["Logs", "Caches"]);
  // A path the tree doesn't hold, or a file, changes nothing.
  assert.equal(graft(t, "/Users/ada/Library/Nope", fresh, 1), false);
  assert.equal(graft(t, "/Users/ada/Movies/holiday.mov", fresh, 1), false);
  assert.equal(graft(t, "/Elsewhere/Caches", fresh, 1), false);
});

test("removePath takes a kept entry out of its parent and its size and items out of every folder above", () => {
  const t = tree();
  const total = t.size;
  assert.equal(removePath(t, "/Users/ada/Library/Caches/pnpm", 900), "entry");
  // pnpm held 900 bytes and 21 items, and itself makes 22 for Caches.
  assert.equal(at(t, "Library", "Caches").size, 100);
  assert.equal(at(t, "Library", "Caches").items, 10);
  assert.equal(t.size, total - 900);
  assertConsistent(t);
  // Library is now smaller than Movies, so Movies comes first.
  assert.deepEqual(t.children.map((c) => c.name), ["Movies", "Library"]);
  assert.equal(removePath(t, "/Users/ada/Movies/holiday.mov", 800), "entry");
  assertConsistent(t);
  assert.equal(removePath(t, "/Users/ada", 1), "root");
  assert.equal(removePath(t, "/Somewhere/else", 1), null);
});

test("removePath takes a folded entry's size off the deepest folder that holds it, and its smaller objects as far as they go", () => {
  const t = tree();
  const total = t.size;
  // `old.log` is among the 600 bytes in Logs' 50 smaller entries.
  assert.equal(removePath(t, "/Users/ada/Library/Logs/old.log", 40), "folded");
  assert.deepEqual(at(t, "Library", "Logs").smaller, { size: 560, items: 49 });
  assert.equal(at(t, "Library", "Logs").size, 560);
  assert.equal(t.size, total - 40);
  assert.equal(t.items, tree().items - 1);
  assertConsistent(t);
  // More than the smaller objects hold: they go to 0, and the folders still lose the size.
  assert.equal(removePath(t, "/Users/ada/Library/Logs/big.log", 1000), "folded");
  assert.equal(at(t, "Library", "Logs").smaller.size, 0);
  assert.equal(at(t, "Library", "Logs").size, 0);
  // A folder with no smaller objects can't hold a folded entry.
  const movies = at(t, "Movies");
  const sized = movies.size;
  assert.equal(removePath(t, "/Users/ada/Movies/new.mov", 5), null);
  assert.equal(movies.size, sized);
});
