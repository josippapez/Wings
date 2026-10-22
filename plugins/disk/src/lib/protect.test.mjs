import test from "node:test";
import assert from "node:assert/strict";
import { protectedReason } from "./protect.ts";

const mounts = ["/", "/Volumes/encrypted"];

test("disks, system folders, home folders and relative paths are refused, each with a sentence that starts with the path", () => {
  assert.equal(protectedReason("/Library", mounts), "/Library is a system folder.");
  for (const path of ["/", "/Volumes/encrypted", "/Volumes/encrypted/", "/System", "/System/Library/Fonts", "/usr", "/usr/bin/env", "/usr/local", "/bin/sh", "/private/var/db/x", "/Library/Apple/y"]) {
    assert.match(protectedReason(path, mounts) ?? "", new RegExp(`^${path.replace(/[/.]/g, "\\$&")}`), path);
  }
  for (const path of ["/Applications", "/Users", "/private", "/private/var", "/var", "/etc", "/tmp", "/opt", "/cores", "/Volumes", "/dev", "/home", "/Users/ada", "/home/ada", "/Users/ada/Library", "/Users/ada/Downloads", "/Users/ada/Public", "relative/path", "~/Downloads"]) {
    assert.notEqual(protectedReason(path, mounts), null, path);
  }
  // `..`, repeated slashes, a trailing slash, case and the links into /private don't get a path past it.
  for (const path of ["/Users/ada/Documents/../Downloads/..", "//Library//", "/library", "/Users/ada/library/", "/var/db/dslocal", "/tmp/../etc"]) {
    assert.notEqual(protectedReason(path, mounts), null, path);
  }
});

test("what is inside a folder that is only refused itself, and inside /usr/local, may be collected", () => {
  for (const path of [
    "/Users/ada/Downloads/old.zip",
    "/Users/ada/Library/Caches",
    "/Users/ada/Desktop/Screenshot.png",
    "/Library/Developer",
    "/Library/Caches/com.example",
    "/Applications/Xcode.app",
    "/usr/local/Cellar",
    "/private/var/folders/zz",
    "/var/folders",
    "/opt/homebrew",
    "/Volumes/encrypted/Backups",
    "/Users/ada/Code/.git",
  ]) {
    assert.equal(protectedReason(path, mounts), null, path);
  }
});
