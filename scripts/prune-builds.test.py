"""Tests for prune-builds.py: `python3 -I scripts/prune-builds.test.py`."""

import importlib.util
import os
import sys
import tempfile
import time
import unittest

# No __pycache__ beside the scripts.
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("prune_builds", os.path.join(os.path.dirname(os.path.abspath(__file__)), "prune-builds.py"))
prune_builds = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prune_builds)

HOUR = 60 * 60


class PruneBuilds(unittest.TestCase):
    def setUp(self):
        self.root = os.path.realpath(tempfile.mkdtemp())
        self.target = os.path.join(self.root, "target")
        self.deps = os.path.join(self.target, "debug", "deps")
        self.caches = os.path.join(self.target, "debug", "incremental")
        os.makedirs(self.deps)
        os.makedirs(self.caches)
        self.now = time.time()

    def put(self, folder, name, hours_ago, text="x"):
        path = os.path.join(folder, name)
        with open(path, "w") as f:
            f.write(text)
        at = self.now - hours_ago * HOUR
        os.utime(path, (at, at))
        return path

    def build(self, crate, hash, hours_ago, files, source="src/lib.rs"):
        """An output group: `files` named from `{crate}-{hash}`, and its dep-info naming `source`."""
        paths = [self.put(self.deps, name.format(f"{crate}-{hash}"), hours_ago) for name in files]
        paths.append(self.put(self.deps, f"{crate}-{hash}.d", hours_ago, f"{self.deps}/{crate}-{hash}.d: {source}\n"))
        return paths

    def run_prune(self):
        return prune_builds.prune(self.target, self.root, self.now)

    def test_an_old_build_of_our_own_crate_goes_and_its_newest_stays(self):
        old = self.build("wings_native", "a" * 16, 10, ["lib{}.rlib", "lib{}.rmeta"])
        newest = self.build("wings_native", "b" * 16, 9, ["lib{}.rlib", "lib{}.rmeta"])
        recent = self.build("wings_native", "c" * 16, 2, ["lib{}.rlib"])
        self.run_prune()
        self.assertFalse(any(map(os.path.exists, old)))
        # Older than six hours, but the newest rmeta: a crate that rarely changes still links it.
        self.assertTrue(all(map(os.path.exists, newest)))
        self.assertTrue(all(map(os.path.exists, recent)))

    def test_an_old_test_binary_goes_with_its_object_files(self):
        old = self.build("wings_native", "1" * 16, 10, ["{}", "{}.wings_native.abc-cgu.0.rcgu.o", "{}.dSYM"])
        newest = self.build("wings_native", "2" * 16, 1, ["{}"])
        self.run_prune()
        self.assertFalse(any(map(os.path.exists, old)))
        self.assertTrue(all(map(os.path.exists, newest)))

    def test_a_dependencys_builds_stay_however_old(self):
        source = os.path.expanduser("~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-pre-0.3.13/src/gpui.rs")
        old = self.build("gpui", "d" * 16, 30, ["lib{}.rlib"], source)
        self.build("gpui", "e" * 16, 1, ["lib{}.rlib"], source)
        # A dependency's generated code is inside the target folder, which isn't ours either.
        generated = self.build("ring", "f" * 16, 30, ["lib{}.rlib"], f"{self.target}/debug/build/ring-1/out/x.rs")
        self.build("ring", "0" * 16, 1, ["lib{}.rlib"])
        self.run_prune()
        self.assertTrue(all(map(os.path.exists, old + generated)))

    def test_a_crate_that_lib_starts_is_kept_apart_from_its_prefix(self):
        # `liblibc-<hash>.rlib` and `libc-<hash>.d` are one build of libc, not of c.
        old = self.build("libc", "3" * 16, 10, [])
        old.append(self.put(self.deps, f"liblibc-{'3' * 16}.rlib", 10))
        self.build("libc", "4" * 16, 1, [])
        self.put(self.deps, f"liblibc-{'4' * 16}.rlib", 1)
        self.run_prune()
        self.assertFalse(any(map(os.path.exists, old)))

    def test_an_old_incremental_cache_goes_but_each_crates_newest_stays(self):
        for name, hours in (("wings_native-0old", 10), ("wings_native-1new", 1), ("wings_agents-0only", 20)):
            os.makedirs(os.path.join(self.caches, name, "s-session"))
            os.utime(os.path.join(self.caches, name), (self.now - hours * HOUR,) * 2)
        self.run_prune()
        self.assertEqual(sorted(os.listdir(self.caches)), ["wings_agents-0only", "wings_native-1new"])

    def test_while_another_build_runs_old_builds_go_but_object_files_stay(self):
        old = self.build("wings_native", "7" * 16, 10, ["{}"])
        self.build("wings_native", "8" * 16, 1, ["{}"])
        binary = self.put(self.deps, f"wings_native-{'9' * 16}", 0)
        unlinked = self.put(self.deps, f"wings_native-{'9' * 16}.wings_native.x-cgu.0.rcgu.o", 0)
        os.utime(binary, (self.now + HOUR,) * 2)
        prune_builds.prune(self.target, self.root, self.now, busy=True)
        self.assertFalse(any(map(os.path.exists, old)))
        self.assertTrue(os.path.exists(unlinked))

    def test_an_object_made_before_its_binarys_last_link_goes(self):
        binary = self.put(self.deps, f"wings_native-{'5' * 16}", 0)
        unlinked = self.put(self.deps, f"wings_native-{'5' * 16}.wings_native.x-cgu.0.rcgu.o", 0)
        linked = self.put(self.deps, f"wings_native-{'6' * 16}.wings_native.x-cgu.0.rcgu.o", 0)
        self.put(self.deps, f"wings_native-{'6' * 16}", 0)
        # Linked an hour after the object was written.
        os.utime(binary, (self.now + HOUR,) * 2)
        self.run_prune()
        self.assertFalse(os.path.exists(unlinked))
        self.assertTrue(os.path.exists(linked))


if __name__ == "__main__":
    unittest.main()
