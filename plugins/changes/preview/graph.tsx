// The commit graph view: every branch's commits with their lanes, from a made-up `git log`.
import { useEffect } from "react";

export const place = "view";
export const fill = true;

const NOW = Math.floor(Date.now() / 1000);
// [hash, parents, author, ago in hours, decoration, subject], newest first, in topological order.
const log: [string, string, string, number, string, string][] = [
  ["a1", "b2 e5", "Ada Lovelace", 1, "HEAD -> refs/heads/main, refs/remotes/origin/main", "Merge pull request #482 from acme/coupon-field"],
  ["b2", "c3", "Grace Hopper", 3, "", "Round shipping rates before they are summed"],
  ["e5", "f6", "Ada Lovelace", 5, "refs/heads/feature/coupon, refs/remotes/origin/feature/coupon", "Add a coupon field to the checkout summary"],
  ["c3", "d4 g7", "Alan Turing", 9, "", "Merge branch 'release/2.13' into main"],
  ["f6", "d4", "Ada Lovelace", 20, "", "Validate coupon codes on the client"],
  ["g7", "h8", "Margaret Hamilton", 30, "refs/heads/release/2.13", "Backport the rounding fix"],
  ["d4", "i9", "Grace Hopper", 50, "tag: refs/tags/v2.14.0", "Release 2.14.0"],
  ["h8", "i9", "Margaret Hamilton", 70, "", "Bump the payment SDK to the latest minor with a long subject that has to be cut off somewhere near the end"],
  ["i9", "j0", "Linus Torvalds", 100, "refs/stash", "WIP on main"],
  ["j0", "", "Ada Lovelace", 5000, "tag: refs/tags/v0.1.0", "Initial commit"],
];
const id = (x: string) => x.padEnd(40, "0");
const out = log.map(([h, p, a, hrs, d, s]) => [id(h), p.split(" ").filter(Boolean).map(id).join(" "), a, NOW - hrs * 3600, NOW - hrs * 3600, d, s].join("\0")).join("\0") + "\0";

export const respond = (m: unknown) => ((m as { type: string }).type === "graph-hello" ? { type: "graph-repo", root: "/work/acme/web", name: "web" } : undefined);

export default function Fixture() {
  // The graph builds its own DOM into the page, as it does when Wings loads graph.js; its git log is the made-up one.
  useEffect(() => {
    (window as unknown as { wings: { exec: unknown } }).wings.exec = () => Promise.resolve({ code: 0, stdout: out, stderr: "" });
    void import("../src/pages/graph");
  }, []);
  return null;
}
