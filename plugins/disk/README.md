# Disk usage

Shows what takes the space on your disks as a map you click into, like DaisyDisk: scan a disk or a folder, find the biggest folders and files, gather them in the Collector and move them to the Trash. Claude can scan too, with its disk tools. Needs Wings 0.1.70.

- **Disks.** Open **Disk usage** from its title bar button, the + menu or the command palette. It starts on your disks, the startup disk first: how full each one is, how much is free, and how much of that macOS can purge on its own (local snapshots and caches, which Finder counts as free). The list is read again every 15 seconds while the tab is open.
- **Scanning.** **Scan** next to a disk scans all of it, which takes up to a minute. Under Folders, **Scan** does the same for your home folder, each project's folder and the folders you star, and a box takes any full path or one that starts with `~`. While it runs the tab shows how much it has found against what it expects (a disk's used space, or the folder's size from an earlier scan), the files scanned so far and the folder it is in. **Stop** ends it. Wings keeps the finished scans, so you can leave the tab and come back, and Claude's scans are in the same list under Scans, where **Open** shows one and **Forget** frees the memory it holds. The scans are gone when Wings restarts.
- **The map.** A finished scan is a ring chart: the folder you are in is the middle, what is in it is the first ring, and what is in those folders the rings after it, each wedge as wide as its size. Click a folder to go into it, or use the path above the map, Back and Forward. The list beside it shows the folder the pointer is on, biggest first. A row's menu has **Add to Collector**, **Reveal in Finder** and **Quick Look**. **Rescan** scans the root again.
- **Detail.** A scan keeps the biggest entries, and folds the rest of each folder into one "smaller items" row, which opens to list every one of them. Going into a folder that the scan kept coarsely scans that folder again in more detail ("Adding detail…").
- **Hidden space.** A scan of a whole disk adds a wedge for the used space it didn't find: **Purgeable** space (**Free it** asks macOS to thin the local Time Machine snapshots, and you may get back less than it says), **Other volumes** that share the disk's space, like VM and Preboot, each **Time Machine snapshot** with a **Delete**, and **Still hidden**: folders Wings can't open, other users' files and system data.
- **Collector.** Drag folders and files onto the bar at the bottom, or choose Add to Collector, to gather what you want to remove. It is kept, so it is still there after a restart. **Move to Trash** waits five seconds, with a Cancel, then moves everything to the Trash and takes it out of the scans. Nothing is deleted: the space comes back once you empty the Trash. A whole disk, a system folder, a home folder and its standard folders (Library, Desktop, Documents, Downloads, Applications, Movies, Music, Pictures, Public) are refused; what is inside them can be collected.
- **Tools for Claude.** `disk__list_disks` lists the disks with their sizes and purgeable space. `disk__folder_usage` (`path`, and optionally `depth` 1 to 4, `limit` 1 to 50 and `fresh`) gives the size of a folder or disk and its biggest entries, and the hidden space for a disk. `disk__largest_files` (`path`, `limit` 1 to 100) lists the biggest files under it. Both reuse a scan under 30 minutes old that holds the path, scan a folder again in more detail when it was kept coarsely, and otherwise scan the path, which then shows in the tab too; a scan of a whole disk takes up to a minute, and a call is cut off after 120 seconds, so one that is still running is used by the next call. `disk__collect` (`paths`, up to 200) adds files and folders to the Collector and says which it refused and why. It never moves anything to the Trash: you review the Collector in the tab and move it from there. A line in the plugin's `mcpInstructions` tells Claude at session start to use these instead of `du`, `find` and `df`.

## Limits

- It scans this Mac's disks and folders. It doesn't scan cloud storage or network shares, and it doesn't scan as administrator: a folder that needs more than your own access, or that macOS keeps private, shows as No access and counts as 0. Give Wings Full Disk Access in System Settings, under Privacy & Security, to see them.
- A size is what a file takes on disk, with a file that has several hard links counted once. APFS clones count in full, so two clones of one file count twice.
- A scan stays on one volume and never follows a symbolic link. Scanning `/` leaves out `/System/Volumes`, which the root already reaches through its firmlinks.
- A scan keeps at most 30,000 entries; the rest of each folder is in its smaller items.
- Moving to the Trash, Quick Look and local snapshots are macOS only, and the Trash needs macOS 15 or newer, which is where the `trash` command first appeared.

## Permissions

- `diskUsage`: reading the sizes of folders and disks in Wings, and listing the disks. It reads sizes, never what a file holds.
- `df -k -P`: the used and free space of each volume, to tell the purgeable space and which volumes share a disk.
- `open -R`: Reveal in Finder.
- `qlmanage -p`: Quick Look.
- `trash`: moving what is in the Collector to the Trash.
- `tmutil listlocalsnapshotdates`, `tmutil deletelocalsnapshots` and `tmutil thinlocalsnapshots`: listing the local snapshots of a disk, deleting the one you pick, and freeing the purgeable space. Each only runs when you ask.
