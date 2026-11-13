//! `agentActivity`: what an OpenCode session is doing, the desktop's `plugin_agent_activity` in
//! `apps/desktop/src-tauri/src/lib.rs`. The service's address and password stay with the app, which asks it.

use serde_json::Value;
use wings_plugins::permissions::may_read_agent_activity;

use super::{js_string, to_json};
use crate::page::plugin::Access;

pub(super) fn agent_activity(access: &Access, params: &Value) -> Result<String, String> {
    if !may_read_agent_activity(&access.plugin) {
        return Err(format!(
            "{} may not read agent activity",
            access.plugin.manifest.id
        ));
    }
    to_json(
        &access
            .env
            .agent_activity(&js_string(params.get("sessionId")))?,
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::page::{
        plugin::testing::{Frame, Rig, barrier, hears, show},
        runtime::fake::{self, Cx, Guest},
    };

    /// A number, which `js_string` makes the text `7`.
    fn asks() -> Value {
        json!({ "sessionId": 7 })
    }

    #[test]
    fn reading_activity_needs_the_permission_and_asks_for_the_session_as_text() {
        let denied = Rig::new(json!({}), vec![]);
        assert_eq!(
            denied.refuses("agentActivity", asks()),
            "probe may not read agent activity"
        );
        assert!(denied.env.asked().is_empty());

        let rig = Rig::new(json!({ "permissions": { "agentActivity": true } }), vec![]);
        rig.env.opencode_knows("7");
        let activity = rig.allows("agentActivity", asks());
        assert_eq!(activity["model"], "gpt-x");
        assert_eq!(activity["tokens"]["cacheRead"], 300);
        assert_eq!(activity["items"][0]["kind"], "prompt");
        assert_eq!(rig.env.asked(), ["7"]);
        assert_eq!(
            rig.refuses("agentActivity", json!({ "sessionId": "ses_gone" })),
            "OpenCode doesn't know that session"
        );
    }

    /// A page that shows what it heard last, `ready` first.
    struct Quiet;

    impl Guest for Quiet {
        fn start(&mut self, cx: &mut Cx<'_>) {
            show(cx, json!("ready"));
        }
        fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, _id: u32, _text: &str) {
            match kind {
                "agentActivity" => show(cx, json!("heard")),
                "broadcast" => show(cx, json!("barrier")),
                _ => {}
            }
        }
    }

    #[test]
    fn activity_events_reach_only_plugins_allowed_to_read_activity() {
        let allowed = Rig::new(json!({ "permissions": { "agentActivity": true } }), vec![]);
        let frame = Frame::start(&allowed.host, &hears("agentActivity"));
        frame.report("the listener", |seen| seen["heard"] == json!([]));
        allowed.host.publish_agent_activity("ses_a");
        assert_eq!(
            frame.report("the event", |seen| seen["heard"] != json!([])),
            json!({ "heard": [{ "sessionId": "ses_a" }] })
        );

        // The barrier is a broadcast sent after the event, so a frame that was going to hear it already has.
        let denied = Rig::new(json!({}), vec![]);
        let quiet = Frame::start(&denied.host, &fake::source("quiet", || Box::new(Quiet)));
        quiet.report("the listener", |seen| seen == "ready");
        denied.host.publish_agent_activity("ses_a");
        let _barrier = Frame::start(&denied.host, &barrier());
        assert_eq!(
            quiet.report("the barrier", |seen| seen != "ready"),
            "barrier"
        );
    }
}
