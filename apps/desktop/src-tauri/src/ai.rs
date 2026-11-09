//! Generated session names and agent summaries. The model work lives in the shared `wings-agents` crate, which the
//! native app uses too; this runs its worker thread and adds the commands Settings calls.

use std::collections::HashMap;

use tauri::{AppHandle, Emitter, Manager};

pub use wings_agents::ai::*;

use crate::AppState;

/// Starts the one thread that makes names and summaries.
pub fn start(app: AppHandle) {
    let _ = std::thread::Builder::new().name("ai".into()).spawn(move || loop {
        let state = app.state::<AppState>();
        match state.ai.work(&state.history, &crate::claude_accounts::dirs()) {
            Ok(Some(Made::Name { id, text })) => {
                let _ = app.emit("ai-names", HashMap::from([(id, text)]));
            }
            Ok(_) => {}
            Err(e) => {
                if cfg!(debug_assertions) {
                    crate::applog::log_line!("[ai] {e}");
                }
            }
        }
    });
}

#[tauri::command(async)]
pub fn ai_status(state: tauri::State<AppState>) -> Status {
    state.ai.status()
}

#[tauri::command(async)]
pub fn ai_set(state: tauri::State<AppState>, engine: Engine, ollama_model: Option<String>) -> Status {
    state.ai.set(Settings { engine, ollama_model });
    state.ai.status()
}
