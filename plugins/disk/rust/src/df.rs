//! The disks overview's numbers: `df -k -P` read for what macOS can purge and which volumes share a disk's space, and the
//! hidden space of a scanned disk.

use serde_json::{Value, json};
use wings_plugin::format::basename;

/// A row of `df -k -P`, in bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct DfRow {
    pub device: String,
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub mount: String,
}

/// The device and the mount point can hold spaces (`map auto_home`, `devices -- file:///...`, a volume named `My Disk`), so
/// a row is matched around its four number columns: three of digits, then a percentage or `-`. The header has none.
fn row_of(line: &str) -> Option<DfRow> {
    let line = line.trim_end();
    let mut tokens: Vec<(usize, &str)> = Vec::new();
    let mut start = None;
    for (at, ch) in line.char_indices().chain([(line.len(), ' ')]) {
        match (start, ch.is_whitespace()) {
            (None, false) => start = Some(at),
            (Some(from), true) => {
                tokens.push((from, &line[from..at]));
                start = None;
            }
            _ => {}
        }
    }
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let percent = |text: &str| text == "-" || text.strip_suffix('%').is_some_and(digits);
    (1..tokens.len()).find_map(|at| {
        let [(_, total), (_, used), (_, available), (_, capacity)]: [(usize, &str); 4] = tokens.get(at..at + 4)?.try_into().ok()?;
        let mount = tokens.get(at + 4)?.0;
        if !(digits(total) && digits(used) && digits(available) && percent(capacity)) {
            return None;
        }
        let (previous_from, previous) = tokens[at - 1];
        let kib = |text: &str| text.parse::<u64>().map_or(u64::MAX, |n| n.saturating_mul(1024));
        Some(DfRow { device: line[..previous_from + previous.len()].to_owned(), total: kib(total), used: kib(used), available: kib(available), mount: line[mount..].to_owned() })
    })
}

/// The rows of `df -k -P`.
pub fn parse_df(text: &str) -> Vec<DfRow> {
    text.lines().filter_map(row_of).collect()
}

/// The free space df reports for the volume mounted at `mount`, which leaves out what macOS can purge, or none when df
/// has no row for it.
pub fn available_of(df: &[DfRow], mount: &str) -> Option<u64> {
    df.iter().find(|row| row.mount == mount).map(|row| row.available)
}

/// The APFS container a device belongs to: `disk3` for `/dev/disk3s5`.
fn container_of(device: &str) -> Option<&str> {
    let rest = device.strip_prefix("/dev/disk")?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    (digits > 0).then(|| &device[..9 + digits])
}

/// A disk as `wings.disks()` lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Disk {
    pub name: String,
    pub mount: String,
    pub fs: String,
    /// `ssd`, `hdd` or `unknown`.
    pub kind: String,
    pub removable: bool,
    pub total: u64,
    pub free: u64,
}

impl Disk {
    pub fn from_value(value: &Value) -> Self {
        let text = |name: &str| value[name].as_str().unwrap_or_default().to_owned();
        Self {
            name: text("name"),
            mount: text("mount"),
            fs: text("fs"),
            kind: text("kind"),
            removable: value["removable"] == true,
            total: value["total"].as_u64().unwrap_or(0),
            free: value["free"].as_u64().unwrap_or(0),
        }
    }
}

/// Another volume that shares a disk's space, like VM and Preboot on an APFS startup disk.
#[derive(Clone, Debug, PartialEq)]
pub struct Volume {
    pub name: String,
    pub mount: String,
    pub used: u64,
}

impl Volume {
    pub fn to_value(&self) -> Value {
        json!({ "name": self.name, "mount": self.mount, "used": self.used })
    }

    pub fn from_value(value: &Value) -> Self {
        Self { name: value["name"].as_str().unwrap_or_default().to_owned(), mount: value["mount"].as_str().unwrap_or_default().to_owned(), used: value["used"].as_u64().unwrap_or(0) }
    }
}

/// A disk as the overview shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct DiskRow {
    pub disk: Disk,
    /// `total - free`: what Finder calls used.
    pub used: u64,
    /// What macOS can free on its own, like local snapshots and caches. Finder counts it as free. 0 when unknown.
    pub purgeable: u64,
    pub others: Vec<Volume>,
}

/// The disks `wings.disks()` lists, with what df adds: the purgeable space, and the other volumes in the same container.
pub fn disk_rows(disks: &[Disk], df: &[DfRow]) -> Vec<DiskRow> {
    // A volume's name is its mount's last part, which the startup disk's `/` has none of.
    let name_of = |mount: &str| if mount == "/" { disks.iter().find(|d| d.mount == "/").map_or("/", |d| d.name.as_str()).to_owned() } else { basename(mount).to_owned() };
    disks
        .iter()
        .map(|disk| {
            let own = df.iter().find(|row| row.mount == disk.mount);
            // Only the volumes of an APFS container share space: two partitions of an exFAT disk don't.
            let container = own.filter(|_| disk.fs == "apfs").and_then(|own| container_of(&own.device));
            let mut others: Vec<Volume> = Vec::new();
            for row in df {
                if container.is_none() || container_of(&row.device) != container || row.mount == disk.mount || row.used == 0 {
                    continue;
                }
                // A scan of `/` reaches the Data volume through its firmlinks.
                if disk.mount == "/" && row.mount == "/System/Volumes/Data" {
                    continue;
                }
                others.push(Volume { name: name_of(&row.mount), mount: row.mount.clone(), used: row.used });
            }
            others.sort_by(|a, b| b.used.cmp(&a.used));
            DiskRow { used: disk.total.saturating_sub(disk.free), purgeable: own.map_or(0, |own| disk.free.saturating_sub(own.available)), others, disk: disk.clone() }
        })
        .collect()
}

/// The used space a scan of a disk didn't find, like DaisyDisk's "hidden space".
#[derive(Clone, Debug, PartialEq)]
pub struct Hidden {
    pub size: u64,
    pub purgeable: u64,
    pub others: Vec<Volume>,
    /// Dates of Time Machine local snapshots, like `2026-10-22-101010`. Their space is part of `purgeable`.
    pub snapshots: Vec<String>,
    /// The rest: folders Wings can't open, other users' files, system data.
    pub rest: u64,
}

impl Hidden {
    pub fn to_value(&self) -> Value {
        json!({ "size": self.size, "purgeable": self.purgeable, "others": self.others.iter().map(Volume::to_value).collect::<Vec<_>>(), "snapshots": self.snapshots, "rest": self.rest })
    }

    pub fn from_value(value: &Value) -> Self {
        Self {
            size: value["size"].as_u64().unwrap_or(0),
            purgeable: value["purgeable"].as_u64().unwrap_or(0),
            others: value["others"].as_array().map_or_else(Vec::new, |list| list.iter().map(Volume::from_value).collect()),
            snapshots: value["snapshots"].as_array().map_or_else(Vec::new, |list| list.iter().filter_map(|date| date.as_str().map(str::to_owned)).collect()),
            rest: value["rest"].as_u64().unwrap_or(0),
        }
    }
}

/// The used space a scan of `disk` didn't find, given the bytes it did (`found`) and the disk's local snapshot dates.
/// `available` is df's free space for it, which leaves out what macOS can purge: the used space counts that as used.
pub fn hidden_space(disk: &DiskRow, available: Option<u64>, found: u64, snapshots: Vec<String>) -> Hidden {
    let used_all = disk.disk.total.saturating_sub(available.unwrap_or(disk.disk.free));
    let size = used_all.saturating_sub(found);
    let rest = size.saturating_sub(disk.purgeable).saturating_sub(disk.others.iter().map(|v| v.used).sum());
    Hidden { size, purgeable: disk.purgeable, others: disk.others.clone(), snapshots, rest }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `df -k -P` on a Mac with an APFS container (disk3) of seven volumes, a second container (disk1), a simulator image, an
    // automount and a device whose name has spaces.
    const FIXTURE: &str = "Filesystem                                    1024-blocks      Used  Available Capacity    Mounted on
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
";

    fn disk(name: &str, mount: &str, total: u64, free: u64) -> Disk {
        Disk { name: name.into(), mount: mount.into(), fs: "apfs".into(), kind: "ssd".into(), removable: false, total, free }
    }

    // What `wings.disks()` said for it: Finder's free space counts what macOS can purge.
    fn startup() -> Disk {
        disk("Macintosh HD", "/", 494_384_795_648, 6_248_427_520)
    }

    fn encrypted() -> Disk {
        disk("encrypted", "/Volumes/encrypted", 494_384_795_648, 6_248_427_520)
    }

    #[test]
    fn df_rows_are_read_whole_whatever_spaces_the_device_and_the_mount_hold_and_the_header_is_no_row() {
        let rows = parse_df(FIXTURE);
        assert_eq!(rows.len(), 14);
        assert_eq!(rows[0], DfRow { device: "/dev/disk3s1s1".into(), total: 482_797_652 * 1024, used: 13_338_136 * 1024, available: 5_333_572 * 1024, mount: "/".into() });
        let auto = rows.iter().find(|r| r.device == "map auto_home").unwrap();
        assert_eq!((auto.mount.as_str(), auto.total), ("/System/Volumes/Data/home", 0));
        let last = rows.last().unwrap();
        assert_eq!(last.device, "devices -- file:///Users/x/Library/Containers/com.apple.CoreDevice/Data/");
        assert_eq!(last.mount, "/Users/x/Library/Developer/CoreDevice/DeviceFS");
        assert_eq!(last.available, 1_073_741_824 * 1024);
        assert_eq!(parse_df("/dev/disk9s1  1000  500  500  50%  /Volumes/My Disk\n")[0].mount, "/Volumes/My Disk");
        assert!(parse_df("").is_empty());
    }

    #[test]
    fn a_disks_purgeable_space_is_finders_free_space_less_dfs_and_its_others_are_the_volumes_of_its_container_biggest_first() {
        let df = parse_df(FIXTURE);
        let rows = disk_rows(&[startup(), encrypted()], &df);
        let (root, other) = (&rows[0], &rows[1]);
        assert_eq!(root.used, 494_384_795_648 - 6_248_427_520);
        assert_eq!(root.purgeable, 6_248_427_520 - 5_333_572 * 1024);
        // Data is reached through the firmlinks of `/`, the disk1 volumes and the simulator image are other disks, and `auto_home` isn't one.
        let names = |row: &DiskRow| row.others.iter().map(|v| v.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(root), ["encrypted", "Preboot", "VM", "479BC299-11CE-488E-89CC-118ADDAB05DB", "Update"]);
        assert_eq!(root.others[0], Volume { name: "encrypted".into(), mount: "/Volumes/encrypted".into(), used: 21_379_292 * 1024 });
        // Seen from another volume of the container, the startup disk is one of the others, by the name Finder gives it, and so is Data.
        assert_eq!(names(other), ["Data", "Macintosh HD", "Preboot", "VM", "479BC299-11CE-488E-89CC-118ADDAB05DB", "Update"]);
        // Without df, or without a row for the disk, there is nothing to add.
        let bare = &disk_rows(&[startup()], &[])[0];
        assert_eq!((bare.purgeable, bare.others.len()), (0, 0));
        assert_eq!(disk_rows(&[disk("Gone", "/Volumes/gone", 100, 40)], &df)[0].purgeable, 0);
        // Two partitions of one exFAT disk don't share space, and two volumes of an APFS container do.
        let parts = parse_df("/dev/disk9s1  1000  500  500  50%  /Volumes/USB\n/dev/disk9s2  1000  200  800  20%  /Volumes/USB 2\n");
        let exfat = Disk { fs: "exfat".into(), ..disk("USB", "/Volumes/USB", 1_000_000, 500_000) };
        assert!(disk_rows(&[exfat], &parts)[0].others.is_empty());
        assert_eq!(names(&disk_rows(&[disk("USB", "/Volumes/USB", 1_000_000, 500_000)], &parts)[0]), ["USB 2"]);
        // Free space under df's, which can't be, is no purgeable space.
        assert_eq!(disk_rows(&[disk("Odd", "/", 1_000_000_000, 1_000_000)], &df)[0].purgeable, 0);
    }

    #[test]
    fn hidden_space_is_what_the_scan_didnt_find_of_the_used_space_less_what_is_purgeable_and_what_other_volumes_use() {
        let root = disk_rows(&[startup()], &parse_df(FIXTURE)).remove(0);
        let found = 400_000_000_000;
        let hidden = hidden_space(&root, Some(5_333_572 * 1024), found, vec!["2026-10-22-101010".into()]);
        assert_eq!(hidden.size, 494_384_795_648 - 5_333_572 * 1024 - found);
        assert_eq!(hidden.purgeable, root.purgeable);
        assert_eq!(hidden.others, root.others);
        assert_eq!(hidden.snapshots, ["2026-10-22-101010"]);
        assert_eq!(hidden.rest, hidden.size.saturating_sub(root.purgeable + root.others.iter().map(|v| v.used).sum::<u64>()));
        // Without df the used space is the disk's own, and a scan that found more than that hides nothing.
        assert_eq!(hidden_space(&root, None, found, vec![]).size, root.used - found);
        let none = hidden_space(&root, None, 1_000_000_000_000, vec![]);
        assert_eq!((none.size, none.rest), (0, 0));
        assert_eq!(Hidden::from_value(&hidden.to_value()), hidden);
    }
}
