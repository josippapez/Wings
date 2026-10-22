import test from "node:test";
import assert from "node:assert/strict";
import { existingFocus, isCoarse, parseDrop } from "./model.ts";

const file = (name, size) => ({ name, size, kind: "file", items: 0 });
const dir = (name, size, more = {}) => ({ name, size, kind: "dir", items: 1, ...more });

// A scan of a disk kept entries of 17 MB and up. The Users folder was scanned again at 7.4 MB: its folders below inherit that.
const scan = { floor: 17_000_000 };
const users = dir("Users", 297e9, { detail: 7_400_000 });

test("a folder is coarse when the floor hides most of it and a scan of it alone would keep more", () => {
  assert.equal(isCoarse(scan, [dir("/", 682e9), dir("tmp", 5e9)]), true);
  assert.equal(isCoarse(scan, [dir("/", 682e9), dir("Applications", 100e9)]), false, "the floor is small next to it");
});

test("a folder inside one scanned again is held to that folder's floor, not the scan's", () => {
  assert.equal(isCoarse(scan, [dir("/", 682e9), users, dir("Code", 5e9)]), true);
  // 7.4 MB * 2000 is under 20 GB, though 17 MB * 2000 is over it: only the scan's floor would call this one coarse.
  assert.equal(isCoarse(scan, [dir("/", 682e9), users, dir("Code", 20e9)]), false);
});

test("a folder scanned again, a file and a folder Wings couldn't open are not coarse", () => {
  assert.equal(isCoarse(scan, [dir("/", 682e9), dir("Code", 5e9, { detail: 125_000 })]), false);
  assert.equal(isCoarse(scan, [dir("/", 682e9), file("a.zip", 5e9)]), false);
  assert.equal(isCoarse(scan, [dir("/", 682e9), dir("Mail", 0, { restricted: true })]), false);
});

test("a folder a scan of it alone would keep no finer floor of is not coarse", () => {
  // 4 KB is the least main keeps, and it is what the folder above was kept to.
  assert.equal(isCoarse({ floor: 4096 }, [dir("/", 1e6), dir("small", 1e6)]), false);
});

const tree = dir("/Users/ada", 10, {
  children: [dir("Code", 6, { children: [file("a.ts", 6)] }), file("notes.txt", 3), dir("Mail", 0, { restricted: true })],
});

test("the folder in the middle is the one asked for, or the nearest above it the tree holds", () => {
  assert.equal(existingFocus(tree, null), "/Users/ada");
  assert.equal(existingFocus(tree, "/Users/ada/Code"), "/Users/ada/Code");
  assert.equal(existingFocus(tree, "/Users/ada/Code/gone/deeper"), "/Users/ada/Code");
  assert.equal(existingFocus(tree, "/Users/ada/Code/a.ts"), "/Users/ada/Code", "a file is not a folder");
  assert.equal(existingFocus(tree, "/Users/ada/Mail"), "/Users/ada", "a folder Wings couldn't open has nothing to show");
  assert.equal(existingFocus(tree, "/Volumes/Backup"), "/Users/ada", "a path outside the scan");
});

test("what is dropped on the Collector is a list of the entries it can take", () => {
  const entry = { path: "/Users/ada/Code", size: 6, kind: "dir" };
  assert.deepEqual(parseDrop(JSON.stringify(entry)), [entry]);
  assert.deepEqual(parseDrop(JSON.stringify([entry, { path: "relative", size: 1, kind: "file" }, { path: "/x", size: 1, kind: "socket" }, null])), [entry]);
  assert.deepEqual(parseDrop("not json"), []);
});
