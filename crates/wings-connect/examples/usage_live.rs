//! Asks once for the usage limits of Claude accounts, by the code the app asks with every 5 seconds, and prints only
//! what came of it: each window's percent used and reset time, or why there is none. Never the token.
//!
//! `cargo run --manifest-path crates/wings-connect/Cargo.toml --example usage_live -- [account ...]`
//!
//! An account is a name, `work` for `~/.claude-work`, or `-` for the default one, which is the one without arguments.
//! The default account is `~/.claude`, or the folder `CLAUDE_CONFIG_DIR` names where this runs, as for the app.

use wings_connect::{
    statusline::Window,
    usage::{Fetched, Usage},
};

fn main() {
    let mut accounts: Vec<Option<String>> = std::env::args().skip(1).map(|a| (a != "-").then_some(a)).collect();
    if accounts.is_empty() {
        accounts.push(None);
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let shown = |window: &Option<Window>| match window {
        Some(Window { used_percentage, resets_at: Some(at) }) => {
            format!("{used_percentage:.1}% used, resets at {at} (in {}h{:02}m)", (at - now) / 3600, (at - now) % 3600 / 60)
        }
        Some(Window { used_percentage, resets_at: None }) => format!("{used_percentage:.1}% used, no reset time"),
        None => "not in the answer".to_string(),
    };
    let usage = Usage::claude();
    for account in accounts {
        let name = account.as_deref().unwrap_or("default");
        match usage.fetch(account.as_deref()) {
            Fetched::Limits { five_hour, seven_day } => {
                println!("{name}: 5-hour {}; weekly {}", shown(&five_hour), shown(&seven_day))
            }
            other => println!("{name}: {other:?}"),
        }
    }
}
