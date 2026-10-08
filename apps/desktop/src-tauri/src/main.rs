// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Claude Code starts `wings --mcp <socket>` as its MCP server. It runs before any Tauri app exists.
    #[cfg(unix)]
    {
        let mut args = std::env::args().skip(1);
        if args.next().as_deref() == Some("--mcp") {
            std::process::exit(wings_lib::bridge::run(args.next()));
        }
    }
    wings_lib::run()
}
