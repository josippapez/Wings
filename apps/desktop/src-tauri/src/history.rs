//! Search over past Claude Code and OpenCode sessions, for the history sheet, the command palette and the
//! `search_history` and `read_session` MCP tools. The search lives in the shared `wings-agents` crate, which the native
//! app uses too; this puts it back under the name the rest of the app uses, and adds the command the sheet calls.

use tauri::{AppHandle, Manager, State};

pub use wings_agents::history::*;

use crate::AppState;

#[tauri::command(async)]
pub fn history_search(app: AppHandle, state: State<AppState>, query: String, filter: Filter, limit: usize) -> Results {
    // OpenCode's sessions load in the background, so this search answers with what is there.
    if state.history.begin_opencode() {
        std::thread::spawn(move || app.state::<AppState>().history.run_opencode());
    }
    let mut results = state.history.search(&crate::claude_accounts::dirs(), &query, &filter, limit);
    state.ai.name_hits(&mut results.sessions);
    results
}
