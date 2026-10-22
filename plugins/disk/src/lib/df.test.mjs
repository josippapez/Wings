import test from "node:test";
import assert from "node:assert/strict";
import { diskRows, hiddenSpace, parseDf } from "./df.ts";

// `df -k -P` on a Mac with an APFS container (disk3) of seven volumes, a second container (disk1), a simulator image, an
// automount and a device whose name has spaces.
const FIXTURE = `Filesystem                                    1024-blocks      Used  Available Capacity    Mounted on
/dev/disk3s1s1                                  482797652  13338136    5333572    72%    /
devfs                                                 211       211          0   100%    /dev
/dev/disk3s6                                    482797652  10489868    5333572    67%    /System/Volumes/VM
/dev/disk3s2                                    482797652  10671540    5333572    67%    /System/Volumes/Preboot
/dev/disk3s4                                    482797652     27100    5333572     1%    /System/Volumes/Update
/dev/disk1s2                                       512000      6164     492420     2%    /System/Volumes/xarts
/dev/disk1s1                                       512000      5964     492420     2%    /System/Volumes/iSCPreboot
/dev/disk1s3                                       512000      2588     492420     1%    /System/Volumes/Hardware
/dev/disk3s5                                    482797652 419870232    5333572    99%    /System/Volumes/Data
map auto_home                                           0         0          0   100%    /System/Volumes/Data/home
/dev/disk3s8                                    482797652     27868    5333572     1%    /Volumes/479BC299-11CE-488E-89CC-118ADDAB05DB
/dev/disk3s7                                    482797652  21379292    5333572    81%    /Volumes/encrypted
/dev/disk6s1                                     18124800  17586672     491796    98%    /Library/Developer/CoreSimulator/Volumes/iOS_24A434
devices -- file:///Users/x/Library/Containers/com.apple.CoreDevice/Data/  1073741824         0 1073741824     0%    /Users/x/Library/Developer/CoreDevice/DeviceFS
`;

const disk = (name, mount, total, free) => ({ name, mount, fs: "apfs", kind: "ssd", removable: false, readOnly: false, total, free });
// What `wings.disks()` said for it: Finder's free space counts what macOS can purge.
const startup = disk("Macintosh HD", "/", 494384795648, 6248427520);
const encrypted = disk("encrypted", "/Volumes/encrypted", 494384795648, 6248427520);

test("df rows are read whole, whatever spaces the device and the mount hold, and the header is no row", () => {
  const rows = parseDf(FIXTURE);
  assert.equal(rows.length, 14);
  assert.deepEqual(rows[0], { device: "/dev/disk3s1s1", total: 482797652 * 1024, used: 13338136 * 1024, available: 5333572 * 1024, mount: "/" });
  const auto = rows.find((r) => r.device === "map auto_home");
  assert.deepEqual([auto.mount, auto.total], ["/System/Volumes/Data/home", 0]);
  const last = rows[rows.length - 1];
  assert.equal(last.device, "devices -- file:///Users/x/Library/Containers/com.apple.CoreDevice/Data/");
  assert.equal(last.mount, "/Users/x/Library/Developer/CoreDevice/DeviceFS");
  assert.equal(last.available, 1073741824 * 1024);
  assert.equal(parseDf("/dev/disk9s1  1000  500  500  50%  /Volumes/My Disk\n")[0].mount, "/Volumes/My Disk");
  assert.deepEqual(parseDf(""), []);
});

test("a disk's purgeable space is Finder's free space less df's, and its others are the volumes of its container, biggest first", () => {
  const [root, other] = diskRows([startup, encrypted], parseDf(FIXTURE));
  assert.equal(root.used, 494384795648 - 6248427520);
  assert.equal(root.purgeable, 6248427520 - 5333572 * 1024);
  // Data is reached through the firmlinks of `/`, the disk1 volumes and the simulator image are other disks, and `auto_home` isn't one.
  assert.deepEqual(root.others.map((v) => v.name), ["encrypted", "Preboot", "VM", "479BC299-11CE-488E-89CC-118ADDAB05DB", "Update"]);
  assert.deepEqual(root.others[0], { name: "encrypted", mount: "/Volumes/encrypted", used: 21379292 * 1024 });
  // Seen from another volume of the container, the startup disk is one of the others, by the name Finder gives it, and so is Data.
  assert.deepEqual(other.others.map((v) => v.name), ["Data", "Macintosh HD", "Preboot", "VM", "479BC299-11CE-488E-89CC-118ADDAB05DB", "Update"]);
  // Without df, or without a row for the disk, there is nothing to add.
  const [bare] = diskRows([startup], []);
  assert.deepEqual([bare.purgeable, bare.others], [0, []]);
  assert.equal(diskRows([disk("Gone", "/Volumes/gone", 100, 40)], parseDf(FIXTURE))[0].purgeable, 0);
  // Two partitions of one exFAT disk don't share space, and two volumes of an APFS container do.
  const parts = parseDf("/dev/disk9s1  1000  500  500  50%  /Volumes/USB\n/dev/disk9s2  1000  200  800  20%  /Volumes/USB 2\n");
  assert.deepEqual(diskRows([{ ...disk("USB", "/Volumes/USB", 1e6, 5e5), fs: "exfat" }], parts)[0].others, []);
  assert.deepEqual(diskRows([disk("USB", "/Volumes/USB", 1e6, 5e5)], parts)[0].others.map((v) => v.name), ["USB 2"]);
  // Free space under df's, which can't be, is no purgeable space.
  assert.equal(diskRows([disk("Odd", "/", 1e9, 1e6)], parseDf(FIXTURE))[0].purgeable, 0);
});

test("hidden space is what the scan didn't find of the used space, less what is purgeable and what other volumes use", () => {
  const [root] = diskRows([startup], parseDf(FIXTURE));
  const found = 400e9;
  const hidden = hiddenSpace(root, 5333572 * 1024, found, ["2026-10-22-101010"]);
  assert.equal(hidden.size, 494384795648 - 5333572 * 1024 - found);
  assert.equal(hidden.purgeable, root.purgeable);
  assert.equal(hidden.others, root.others);
  assert.deepEqual(hidden.snapshots, ["2026-10-22-101010"]);
  assert.equal(hidden.rest, Math.max(0, hidden.size - root.purgeable - root.others.reduce((sum, v) => sum + v.used, 0)));
  // Without df the used space is the disk's own, and a scan that found more than that hides nothing.
  assert.equal(hiddenSpace(root, null, found, []).size, root.used - found);
  assert.deepEqual([hiddenSpace(root, null, 1e12, []).size, hiddenSpace(root, null, 1e12, []).rest], [0, 0]);
});
