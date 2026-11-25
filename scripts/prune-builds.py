"""Removes the build output cargo leaves behind and never uses again: `python3 -I prune-builds.py <target>...`.

- Old builds of this repo's own crates. A version bump or a dependency change gives a crate a new hash, and cargo keeps
  the old outputs: each left the app's 380 MB rlib, its 210 MB test binary and that binary's objects, 16 times in a day.
  A crate's outputs untouched for six hours go, except the newest of each kind, which a crate that rarely changes still
  links. Every build of a crate rewrites its outputs, so one in use is never that old.
- Incremental caches untouched for six hours.
- Object files a debug binary no longer links. macOS debug builds keep each binary's objects beside it for its debug
  info, and every incremental build writes new ones without removing the old: 23 GB of them filled the disk in a day.

With `--busy`, another build runs: it may be about to link an object file its binary's last link didn't use, so those
stay. An old build is still removed: one untouched for six hours that isn't its crate's newest is only in use when a
build of that variant starts after six hours without one, and then a rerun builds it again.
"""

import os
import re
import shutil
import sys
import time

STALE = 6 * 60 * 60
# An object of the binary `<name>-<hash>` made over five minutes before that binary was last linked isn't one it links: a
# build writes the objects it links within the minute or two it runs.
MARGIN = 5 * 60
OUTPUT = re.compile(r"(lib)?(\w+)-([0-9a-f]{16})(\..*)?")
KINDS = {"": "binary", ".rlib": "rlib", ".rmeta": "rmeta", ".dylib": "dylib"}


def size(path):
    # The other build slot may remove what this one is counting.
    try:
        if os.path.isdir(path) and not os.path.islink(path):
            return sum(size(os.path.join(path, name)) for name in os.listdir(path))
        st = os.lstat(path)
    except FileNotFoundError:
        return 0
    return st.st_blocks * 512 if st.st_nlink == 1 else 0


def remove(path):
    freed = size(path)
    if os.path.isdir(path) and not os.path.islink(path):
        shutil.rmtree(path, ignore_errors=True)
    else:
        try:
            os.unlink(path)
        except FileNotFoundError:
            return 0
    return freed


def own(folder, group, root):
    """Whether the output `group` (`<name>-<hash>`) was built from this repo's sources, by its dep-info: cargo hands rustc
    a path inside the repo for its own crates, and one in `~/.cargo` for the rest."""
    try:
        with open(os.path.join(folder, group + ".d"), errors="replace") as info:
            line = info.readline()
    except FileNotFoundError:
        return False
    sources = line.split(": ", 1)[1].split() if ": " in line else []
    if not sources:
        return False
    first = sources[0]
    return not os.path.isabs(first) or (first.startswith(root + os.sep) and f"{os.sep}target{os.sep}" not in first)


def old_builds(folder, root, now):
    """The output groups of this repo's crates in `folder` (a `deps` or `examples`) nothing will link again."""
    groups = {}
    for name in os.listdir(folder):
        match = OUTPUT.fullmatch(name)
        if not match:
            continue
        lib, crate, hash, rest = match.groups()
        ext = rest or ""
        # `liblibc-<hash>.rlib` is the crate libc, `libc-<hash>.d` too.
        if lib and ext not in (".rlib", ".rmeta", ".dylib", ".a"):
            crate = "lib" + crate
        group = groups.setdefault(f"{crate}-{hash}", {"crate": crate, "paths": [], "newest": 0.0, "kinds": {}})
        path = os.path.join(folder, name)
        mtime = os.lstat(path).st_mtime
        group["paths"].append(path)
        group["newest"] = max(group["newest"], mtime)
        if ext in KINDS:
            group["kinds"][KINDS[ext]] = mtime
    newest = {}
    for key, group in groups.items():
        for kind, mtime in group["kinds"].items():
            mark = (group["crate"], kind)
            if mark not in newest or mtime > newest[mark][1]:
                newest[mark] = (key, mtime)
    kept = {key for key, _ in newest.values()}
    return [group for key, group in groups.items() if key not in kept and now - group["newest"] > STALE and own(folder, key, root)]


def old_caches(folder, now):
    """The incremental caches in `folder` untouched for six hours, but each crate's newest."""
    caches = [(name, os.path.join(folder, name)) for name in os.listdir(folder)]
    mtimes = {path: os.lstat(path).st_mtime for _, path in caches}
    newest = {}
    for name, path in caches:
        crate = name.rsplit("-", 1)[0]
        if crate not in newest or mtimes[path] > mtimes[newest[crate]]:
            newest[crate] = path
    return [path for _, path in caches if path not in newest.values() and now - mtimes[path] > STALE]


def unlinked_objects(folder):
    """The object files in `folder` no binary there links."""
    files = os.listdir(folder)
    linked = {f: os.stat(os.path.join(folder, f)).st_mtime for f in files if re.fullmatch(r"\w+-[0-9a-f]{16}", f)}
    for f in files:
        binary = f.split(".", 1)[0]
        if not f.endswith(".o") or binary not in linked:
            continue
        path = os.path.join(folder, f)
        try:
            # The inode's change time: an object an incremental build reuses is linked in again, which sets it.
            if os.lstat(path).st_ctime < linked[binary] - MARGIN:
                yield path
        # The other build slot pruned it first.
        except FileNotFoundError:
            pass


def prune(target, root, now, busy=False):
    """Removes what `target` (a cargo target folder) holds that nothing uses, and says how many bytes that freed."""
    freed = 0
    for profile in ("debug", "release"):
        for kind in ("deps", "examples"):
            folder = os.path.join(target, profile, kind)
            if not os.path.isdir(folder):
                continue
            # A file the other build slot removed while this one read the folder: the next build prunes the rest.
            try:
                for group in old_builds(folder, root, now):
                    freed += sum(remove(path) for path in group["paths"])
                if profile == "debug" and not busy:
                    freed += sum(remove(path) for path in list(unlinked_objects(folder)))
            except FileNotFoundError:
                continue
        caches = os.path.join(target, profile, "incremental")
        if os.path.isdir(caches):
            try:
                freed += sum(remove(path) for path in old_caches(caches, now))
            except FileNotFoundError:
                pass
    return freed


if __name__ == "__main__":
    busy = "--busy" in sys.argv[1:]
    root = os.getcwd()
    freed = sum(prune(os.path.abspath(target), root, time.time(), busy) for target in sys.argv[1:] if target != "--busy")
    if freed > 100_000_000:
        print(f"build.sh: removed {freed / 1e9:.1f} GB of old build output", file=sys.stderr)
