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
    use serde_json::json;

    use crate::page::plugin::testing::{Frame, PRELUDE, Rig};

    /// A number, which the desktop's wrapper turns into the text `7`.
    const ASKS: &str = "return await wings.agentActivity(7);";

    #[test]
    fn reading_activity_needs_the_permission_and_asks_for_the_session_as_text() {
        let denied = Rig::new(json!({}), vec![]);
        assert_eq!(denied.refused(ASKS), "probe may not read agent activity");
        assert!(denied.env.asked().is_empty());

        let rig = Rig::new(json!({ "permissions": { "agentActivity": true } }), vec![]);
        rig.env.opencode_knows("7");
        let activity = rig.allowed(ASKS);
        assert_eq!(activity["model"], "gpt-x");
        assert_eq!(activity["tokens"]["cacheRead"], 300);
        assert_eq!(activity["items"][0]["kind"], "prompt");
        assert_eq!(rig.env.asked(), ["7"]);
        assert_eq!(
            rig.refused("return await wings.agentActivity('ses_gone');"),
            "OpenCode doesn't know that session"
        );
    }

    #[test]
    fn activity_events_reach_only_plugins_allowed_to_read_activity() {
        let allowed = Rig::new(json!({ "permissions": { "agentActivity": true } }), vec![]);
        let frame = Frame::start(
            &allowed.host,
            &format!(
                "{PRELUDE} const seen = []; wings.onAgentActivity(event => {{ seen.push(event); show(seen); }}); show(seen);"
            ),
        );
        frame.report("the listener", |seen| seen == &json!([]));
        allowed.host.publish_agent_activity("ses_a");
        assert_eq!(
            frame.report("the event", |seen| seen != &json!([])),
            json!([{ "sessionId": "ses_a" }])
        );

        // The barrier is a broadcast sent after the event, so a frame that was going to hear it already has.
        let denied = Rig::new(json!({}), vec![]);
        let quiet = Frame::start(
            &denied.host,
            &format!(
                "{PRELUDE} wings.onAgentActivity(() => show('heard')); wings.onBroadcast(() => show('barrier')); show('ready');"
            ),
        );
        quiet.report("the listener", |seen| seen == "ready");
        denied.host.publish_agent_activity("ses_a");
        let _barrier = Frame::start(&denied.host, "wings.broadcast({});");
        assert_eq!(
            quiet.report("the barrier", |seen| seen != "ready"),
            "barrier"
        );
    }
}
