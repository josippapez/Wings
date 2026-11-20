# Builds

Build and run tests through the queue, from the main checkout: `sh scripts/build.sh <command>`, for example `sh scripts/build.sh cargo test --manifest-path apps/native/Cargo.toml --features test-support --lib <filter>` or `sh scripts/build.sh sh plugins/build-rust.sh claude-chat`. It runs one build at a time in the checkout's own target folders (`apps/native/target`, `plugins/target`), waits for a build already running, and drops the incremental caches when under 8 GB is free. Every other place that built made a target folder of several GB of its own, and the disk filled up.

- Subagents build and test their own code through it: compile, run the tests they wrote, and hand back working code. A hook refuses a subagent's `cargo build|test|run|check|clippy`, `build-rust.sh`, `package-mac-app.sh` and `pnpm shot|test|build` outside `scripts/build.sh`.
- Another agent may be editing the same checkout: a compile error in a file you didn't touch is theirs, so say so instead of fixing it.
- Never set `CARGO_TARGET_DIR`, and don't build in a worktree (the script refuses).
