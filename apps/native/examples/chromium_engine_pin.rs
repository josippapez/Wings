//! Prints the tree hash `web::engine::PINNED.tree` holds for a Chromium engine archive: the bundle Wings unpacks the
//! archive to as its install does, hashed as every start of Chromium checks it, with how long that check takes and
//! whether it passes against the pin this build holds. `apps/native/scripts/package-chromium-engine.sh` runs it:
//!   cargo run --release --manifest-path apps/native/Cargo.toml --example chromium_engine_pin -- <archive>

use std::time::Instant;

use wings_native::web::engine;

fn main() {
    let Some(archive) = std::env::args_os().nth(1) else {
        eprintln!("usage: chromium_engine_pin <archive>");
        std::process::exit(2);
    };
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let (bundle, tree) = engine::tree_of(archive.as_ref(), scratch.path()).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });
    println!("tree:     {}", tree.hash);
    println!("files:    {} bytes in {} files and folders", tree.bytes, tree.entries);
    for run in 1..=3 {
        let at = Instant::now();
        let checked = engine::verify_bundle(&bundle);
        let verdict = if checked.is_ok() { "matches" } else { "doesn't match" };
        println!("check {run}:  {verdict} the pin in this build, in {} ms", at.elapsed().as_millis());
    }
}
