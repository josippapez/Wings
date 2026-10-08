// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Claude Code starts `wings --mcp <socket>` as its MCP server, and `wings plugin ...` is the CLI. Both run
    // before any Tauri app exists.
    #[cfg(unix)]
    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        match args.first().map(String::as_str) {
            Some("--mcp") => std::process::exit(wings_lib::bridge::run(args.get(1).cloned())),
            Some("plugin") => std::process::exit(wings_lib::cli::run(&args[1..])),
            _ => {}
        }
    }
    wings_lib::run()
}
