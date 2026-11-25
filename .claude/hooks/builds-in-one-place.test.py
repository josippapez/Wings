"""Tests for builds-in-one-place.py: `python3 -I .claude/hooks/builds-in-one-place.test.py`."""

import importlib.util
import os
import sys
import unittest

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("guard", os.path.join(os.path.dirname(os.path.abspath(__file__)), "builds-in-one-place.py"))
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


class Guard(unittest.TestCase):
    def test_a_build_outside_the_queue_is_refused(self):
        for command in [
            "cargo test",
            "cd apps/native && cargo build",
            "RUST_LOG=debug cargo test --lib x",
            "timeout 600 cargo test",
            "nice -n 10 cargo +nightly build",
            "sh plugins/build-rust.sh claude-chat",
            "cd plugins && sh build-rust.sh --quick hud",
            "pnpm --dir plugins shot /tmp/x --hud",
            "bash -c 'cd apps/native && cargo check'",
            "echo start\ncargo clippy",
            "(cd apps/native && cargo run --example shot)",
            "out=$(cargo build 2>&1)",
        ]:
            self.assertEqual(guard.verdict(command), "deny", command)

    def test_the_queue_and_commands_that_only_mention_a_build_pass(self):
        for command in [
            "sh scripts/build.sh cargo test --manifest-path apps/native/Cargo.toml --lib x",
            "sh scripts/build.sh sh plugins/build-rust.sh claude-chat",
            "cd /repo && sh scripts/build.sh pnpm --dir plugins shot /tmp/x --hud",
            'rg -n "cargo test" .claude',
            "grep -n 'build-rust.sh' plugins/README.md",
            'git commit -m "Builds: cargo test runs faster"',
            "cat <<'EOF' > notes.md\ncargo test runs here\nsh build-rust.sh too\nEOF\necho written",
            "cargo fmt --manifest-path apps/native/Cargo.toml -- --check",
            "cargo metadata --format-version 1 --no-deps",
        ]:
            self.assertEqual(guard.verdict(command), "allow", command)


if __name__ == "__main__":
    unittest.main()
