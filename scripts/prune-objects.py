"""Removes the object files no binary links from cargo's debug folders: `python3 -I prune-objects.py <target/debug>...`.

Debug builds on macOS keep each binary's object files beside it for its debug info, and every incremental build writes
new ones without removing the old: 23 GB of them filled the disk in a day. An object of the binary `<name>-<hash>` made
over five minutes before that binary was last linked isn't one it links: a build writes the objects it links
within the minute or two it runs. With half an hour, agents relinking every few minutes left 9.5 GB unpruned.
"""

import os
import re
import sys

MARGIN = 5 * 60

freed = 0
for target in sys.argv[1:]:
    for kind in ("deps", "examples"):
        folder = os.path.join(target, kind)
        try:
            files = os.listdir(folder)
        except FileNotFoundError:
            continue
        linked = {f: os.stat(os.path.join(folder, f)).st_mtime for f in files if re.fullmatch(r"\w+-[0-9a-f]{16}", f)}
        for f in files:
            binary = f.split(".", 1)[0]
            if not f.endswith(".o") or binary not in linked:
                continue
            path = os.path.join(folder, f)
            try:
                st = os.lstat(path)
                # The inode's change time: an object an incremental build reuses is linked in again, which sets it.
                if st.st_ctime < linked[binary] - MARGIN:
                    os.unlink(path)
                    freed += st.st_blocks * 512 if st.st_nlink == 1 else 0
            # The other build slot pruned it first.
            except FileNotFoundError:
                pass
if freed > 100_000_000:
    print(f"build.sh: removed {freed / 1e9:.1f} GB of object files no binary links", file=sys.stderr)
