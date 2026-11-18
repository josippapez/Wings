# Builds

Build and run tests only in the main session, in the main checkout (`apps/native/target`, `plugins/target`). Every other place that builds makes a target folder of several GB of its own, and the disk filled up.

- Subagents write code and hand it back with what to build and test. A hook refuses their `cargo build|test|run|check|clippy`, `build-rust.sh`, `package-mac-app.sh` and `pnpm shot|test|build`.
- Never set `CARGO_TARGET_DIR`, and don't build in a worktree.
- When the disk runs low, delete `apps/native/target/debug/incremental` first: it is a cache, and the next build makes it again.
