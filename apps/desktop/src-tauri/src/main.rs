// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Claude Code starts `wings --mcp <socket>` as its MCP server and can run `wings statusline` as its status
    // line and `wings hook` as a hook, `wings journal` as the journal's sink, and `wings plugin ...` is the CLI.
    // `wings ptyd` is the terminal host. All run before any Tauri app exists.
    #[cfg(unix)]
    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        #[cfg(target_os = "macos")]
        if args.first().is_some_and(|a| a == "ptyd") {
            std::process::exit(wings_lib::ptyd(&args[1..]));
        }
        if let Some(code) = wings_connect::run_command(&args, env!("CARGO_PKG_VERSION")) {
            std::process::exit(code);
        }
    }
    wings_lib::run()
}
