import test from "node:test";
import assert from "node:assert/strict";
import { tree, root, homePath } from "../../preview/_tree.ts";
import { TAU, hitTest, layout } from "./sunburst.ts";
import { parentPath } from "./tree.ts";

const near = (actual, expected, message = "") => assert.ok(Math.abs(actual - expected) < 1e-9, `${message} ${actual} is not ${expected}`);
const options = { rings: 6, hidden: null, minAngle: 0.012 };
const span = (s) => s.a1 - s.a0;
const file = (name, size) => ({ name, size, kind: "file", items: 0 });
const dir = (name, children, smaller) => ({ name, size: children.reduce((n, c) => n + c.size, smaller?.size ?? 0), kind: "dir", items: children.length, children, ...(smaller ? { smaller } : {}) });
const ringZero = (map) => map.segments.filter((s) => s.ring === 0);

test("the first ring adds up to a full circle, with the hidden space and without it", () => {
  for (const focus of [root, homePath, `${homePath}/Library`]) {
    const plain = layout(tree, focus, options);
    near(ringZero(plain).reduce((n, s) => n + span(s), 0), TAU, focus);

    const withHidden = layout(tree, focus, { ...options, hidden: 52e9 });
    const first = ringZero(withHidden);
    near(first.reduce((n, s) => n + span(s), 0), TAU, `${focus} with hidden space`);
    // The hidden space is the last wedge, and its share of everything in the ring.
    const wedge = first[first.length - 1];
    assert.deepEqual(wedge.target, { kind: "hidden" });
    near(span(wedge), (TAU * 52e9) / (ringZero(plain).reduce((n, s) => n + s.size, 0) + 52e9), focus);
  }
});

test("children stay inside their parent's span, and a folder that couldn't be opened gets no wedge", () => {
  const map = layout(tree, root, { ...options, hidden: 52e9 });
  assert.ok(map.rings >= 4);
  assert.equal(map.byPath.has("/Users/guest"), false);
  assert.equal(map.byPath.has(`${homePath}/Library/Mail`), false);

  let inside = 0;
  map.segments.forEach((s, i) => {
    // Ordered by ring, then by angle, and no wedge starts before the one before it ends.
    const before = map.segments[i - 1];
    if (before?.ring === s.ring) assert.ok(before.a1 <= s.a0 + 1e-9, `${s.name} starts before ${before.name} ends`);
    else if (before) assert.equal(before.ring + 1, s.ring);
    assert.ok(s.a0 >= -1e-9 && s.a1 <= TAU + 1e-9 && s.a1 > s.a0, s.name);
    if (s.ring === 0) return;
    // A folder's children and its smaller objects are in the ring after it.
    const folder = s.target.kind === "smaller" ? s.target.folder : parentPath(s.target.path);
    const parent = map.byPath.get(folder);
    assert.ok(parent, `${s.name} has no wedge for ${folder}`);
    assert.equal(parent.ring, s.ring - 1);
    assert.ok(s.a0 >= parent.a0 - 1e-9 && s.a1 <= parent.a1 + 1e-9, `${s.name} is outside ${parent.name}`);
    inside++;
  });
  assert.ok(inside > 20);
});

test("a child narrower than minAngle is folded into its folder's smaller wedge, and what is under it goes too", () => {
  const logs = dir("logs", [file("big.log", 600), file("mid.log", 300), dir("old", [file("a.log", 2), file("b.log", 2)])], { size: 96, items: 7 });
  const top = dir("/data", [logs, file("rest.bin", 1000)]);
  const all = layout(top, "/data", { rings: 3, hidden: null, minAngle: 0 });
  assert.ok(all.byPath.has("/data/logs/old") && all.byPath.has("/data/logs/old/a.log"));

  // "old" is 4 of the 1000 bytes in "logs", which is half the circle: about 0.0126 rad.
  const map = layout(top, "/data", { rings: 3, hidden: null, minAngle: 0.02 });
  assert.equal(map.byPath.has("/data/logs/old"), false);
  assert.equal(map.byPath.has("/data/logs/old/a.log"), false);
  const smaller = map.segments.find((s) => s.target.kind === "smaller");
  assert.deepEqual(smaller.target, { kind: "smaller", folder: "/data/logs" });
  // Its own 96 bytes in 7 items, and the 4 bytes of "old" and the 3 items it is, are counted there.
  assert.equal(smaller.size, 100);
  assert.equal(smaller.items, 7 + 3);
  const folder = map.byPath.get("/data/logs");
  near(span(smaller), (span(folder) * 100) / 1000);
  // It is the last wedge of the folder's span, and the wedges in that ring add up to the folder.
  near(smaller.a1, folder.a1);
  const ring = map.segments.filter((s) => s.ring === 1);
  assert.equal(ring[ring.length - 1], smaller);
  assert.equal(ring.reduce((n, s) => n + s.size, 0), folder.size);
});

test("hitTest finds the wedge under a point, the centre, and nothing outside", () => {
  // /r: a (50, holding a1 of 30 and a2 of 20), b (30, a file) and c (20, an empty folder), round a circle at (200, 150).
  const top = dir("/r", [dir("a", [file("a1", 30), file("a2", 20)]), file("b", 30), { name: "c", size: 20, kind: "dir", items: 0 }]);
  const geometry = { cx: 200, cy: 150, inner: 30, ringWidth: 20 };
  /** The point `turns` of the way round the circle from 12 o'clock, clockwise, `radius` from its centre. */
  const at = (turns, radius) => [geometry.cx + radius * Math.sin(turns * TAU), geometry.cy - radius * Math.cos(turns * TAU)];
  const found = (map, turns, radius) => {
    const hit = hitTest(map, geometry, ...at(turns, radius));
    return typeof hit === "string" || hit === null ? hit : hit.name;
  };

  // a takes the first half of the ring, b the next 30% and c the last 20%. Past a, a1 takes 30% of the circle and a2 20%.
  const map = layout(top, "/r", { rings: 2, hidden: null, minAngle: 0 });
  assert.equal(map.rings, 2);
  assert.equal(found(map, 0, 40), "a");
  assert.equal(found(map, 0.25, 40), "a");
  assert.equal(found(map, 0.4999, 40), "a");
  assert.equal(found(map, 0.5001, 40), "b");
  assert.equal(found(map, 0.7, 40), "b");
  assert.equal(found(map, 0.9, 40), "c");
  assert.equal(found(map, 0.99999, 40), "c");
  assert.equal(found(map, 0.15, 60), "a1");
  assert.equal(found(map, 0.4, 60), "a2");
  // b and c have nothing past them, and the map ends at 70.
  assert.equal(found(map, 0.65, 60), null);
  assert.equal(found(map, 0.25, 70.01), null);
  assert.equal(found(map, 0.25, 400), null);
  // The circle in the middle, wherever in it.
  assert.equal(found(map, 0.25, 0), "center");
  assert.equal(found(map, 0.7, 29.9), "center");
  assert.equal(found(map, 0.7, 30.5), "b");

  // With as much hidden space as there is in the folder, the ring is half hidden: a, b and c take the first half.
  const withHidden = layout(top, "/r", { rings: 2, hidden: 100, minAngle: 0 });
  assert.equal(found(withHidden, 0.2, 40), "a");
  assert.equal(found(withHidden, 0.3, 40), "b");
  assert.equal(found(withHidden, 0.45, 40), "c");
  assert.deepEqual(hitTest(withHidden, geometry, ...at(0.7, 40)).target, { kind: "hidden" });
});
