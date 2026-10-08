//! Requests from the `wings plugin` CLI (cli.rs), over the same socket as the MCP bridge. They change plugins
//! through the same code as the Plugins sheet, and a plugin still only turns on once you approve it in Wings.

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::{plugin_store::PluginView, AppState, Res};

/// `None` when `op` isn't a CLI request.
pub fn handle(app: &AppHandle, op: &str, request: &Value) -> Option<Res<Value>> {
    let arg = |key: &str| request.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    // The UI reloads its list, and opens the approval for a plugin that needs one.
    let changed = |id: &str, review: bool| {
        let _ = app.emit_to("main", "plugins-changed", json!({ "id": id, "review": review }));
    };
    let installed = |view: PluginView| {
        changed(&view.manifest.id, !(view.enabled && view.approved));
        json!(view)
    };
    Some(match op {
        "plugins" => Ok(json!(app.state::<AppState>().plugins.lock().unwrap().list())),
        "install" => {
            let source = arg("source");
            // The CLI sends files as absolute paths; anything else is a GitHub link.
            let view = if std::path::Path::new(&source).is_absolute() { crate::install_file(app, &source) } else { crate::install_github(app, &source) };
            view.map(installed)
        }
        "update" => crate::update_plugin(app, &arg("plugin")).map(installed),
        "enable" => {
            let id = arg("plugin");
            let grant = app.state::<AppState>().plugins.lock().unwrap().current_grant(&id);
            match grant {
                Ok(Some(grant)) => crate::set_enabled(app, &id, true, Some(grant)).map(|view| {
                    changed(&id, false);
                    json!(view)
                }),
                Ok(None) => {
                    changed(&id, true);
                    Err(format!("{id} needs your approval first. Review its access in Wings, which has it open now."))
                }
                Err(e) => Err(e),
            }
        }
        "disable" => {
            let id = arg("plugin");
            crate::set_enabled(app, &id, false, None).map(|view| {
                changed(&id, false);
                json!(view)
            })
        }
        "remove" => {
            let id = arg("plugin");
            crate::remove_plugin(app, &id).map(|()| {
                changed(&id, false);
                Value::Null
            })
        }
        _ => return None,
    })
}
