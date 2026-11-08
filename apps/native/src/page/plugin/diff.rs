use super::{Done, PluginHost, Shared};
use crate::{
    diff::{self, View},
    page::runtime::{Delivery, Message, Reply, Role},
};
use serde_json::{Value, json};
use std::sync::{Arc, atomic::Ordering};

impl Shared {
    pub(super) fn diff_call(
        self: &Arc<Self>,
        frame: u64,
        method: &str,
        params: Value,
        reply: Reply,
    ) {
        let Some(access) = self.access.clone() else {
            return reply.err(format!("{} is turned off or not installed", self.config.id));
        };
        if let Err(error) = super::calls::needs(&access.plugin, "diff") {
            return reply.err(error);
        }
        let this = self.clone();
        let opening = method == "openDiff";
        self.off_thread(reply, move |_| {
            let update = diff::clean_update(&params)?;
            if opening {
                let title = diff::text(params.get("title"), 200);
                if title.is_empty() {
                    return Err("openDiff needs a title".into());
                }
                let id = format!(
                    "{}:{}",
                    this.config.id,
                    this.next_diff.fetch_add(1, Ordering::Relaxed)
                );
                // Serialize creation with disposal so a stopped opener can't leave an orphan viewer.
                let frames = this.frames.lock().unwrap();
                if !frames.iter().any(|open| open.id == frame) {
                    return Err("The plugin frame was closed".into());
                }
                this.diffs.lock().unwrap().insert(id.clone(), (frame, None));
                let view = View {
                    id: id.clone(),
                    plugin_id: this.config.id.clone(),
                    title,
                    subtitle: diff::text(params.get("subtitle"), 200),
                    cwd: diff::text(params.get("cwd"), 1000),
                    refresh: params.get("refresh") == Some(&Value::Bool(true)),
                    update,
                };
                if let Err(error) = access.env.open_diff(view) {
                    this.diffs.lock().unwrap().remove(&id);
                    return Err(error);
                }
                Ok(json!({ "id": id }).to_string())
            } else {
                let id = params
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("undefined");
                if this.diffs.lock().unwrap().contains_key(id) {
                    access.env.update_diff(&this.config.id, id, update)?;
                }
                Ok("null".into())
            }
        });
    }

    pub(super) fn close_frame_diffs(&self, frame: u64) {
        // Creation takes frames before diffs, so removing membership must use the same order.
        self.frames.lock().unwrap().retain(|open| open.id != frame);
        let ids: Vec<_> = self
            .diffs
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, (owner, _))| *owner == frame)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.close_diff(&id, true);
        }
    }

    pub(super) fn close_all_diffs(&self) {
        let ids: Vec<_> = self.diffs.lock().unwrap().keys().cloned().collect();
        for id in ids {
            self.close_diff(&id, true);
        }
    }

    fn close_diff(&self, id: &str, request_close: bool) {
        let Some((_, token)) = self.diffs.lock().unwrap().remove(id) else {
            return;
        };
        if let Some(token) = token {
            self.settle_action(token, Some("The diff viewer was closed".into()));
        }
        if let Some(access) = &self.access
            && request_close
        {
            access.env.close_diff(&self.config.id, id);
        }
        if let Some(main) = self
            .frames
            .lock()
            .unwrap()
            .iter()
            .find(|frame| frame.role == Role::Main)
        {
            main.inbox
                .send(Message::DiffClosed, 0, json!({ "diffId": id }).to_string());
        }
    }
}

impl PluginHost {
    pub fn diff_closed(&self, id: &str) {
        self.shared.close_diff(id, false);
    }

    pub fn cancel_diff_refresh(&self, id: &str) {
        let token = self
            .shared
            .diffs
            .lock()
            .unwrap()
            .get_mut(id)
            .and_then(|(_, token)| token.take());
        if let Some(token) = token {
            self.shared
                .settle_action(token, Some("The plugin didn't respond".into()));
        }
    }

    pub fn send_diff_refresh(
        &self,
        id: &str,
        done: impl FnOnce(Result<(), String>) + Send + 'static,
    ) {
        let main = self
            .shared
            .frames
            .lock()
            .unwrap()
            .iter()
            .find(|frame| frame.role == Role::Main)
            .map(|frame| frame.inbox.clone());
        let Some(main) = main else {
            return done(Err("The plugin is not running".into()));
        };
        let mut diffs = self.shared.diffs.lock().unwrap();
        let Some((_, pending)) = diffs.get_mut(id) else {
            return done(Err("The diff viewer was closed".into()));
        };
        let token = {
            let mut actions = self.shared.actions.lock().unwrap();
            actions.last += 1;
            let token = actions.last;
            actions.waiting.insert(token, Box::new(done) as Done);
            token
        };
        *pending = Some(token);
        drop(diffs);
        if main.send(
            Message::DiffRefresh,
            token,
            json!({ "diffId": id }).to_string(),
        ) != Delivery::Queued
        {
            self.shared
                .settle_action(token, Some("The plugin didn't respond".into()));
        }
    }

    /// Hands a reply typed in the viewer `id` under the thread whose first comment is `reply_to` to the main frame's
    /// `onReply`. `done` gets the plugin's error, or nothing once it has posted the reply.
    pub fn send_reply(
        &self,
        id: &str,
        reply_to: i64,
        body: &str,
        done: impl FnOnce(Result<(), String>) + Send + 'static,
    ) {
        let text = json!({ "diffId": id, "replyTo": reply_to, "body": body }).to_string();
        self.send_action_message(Message::Reply, text, done);
    }

    /// Opens a link from a comment the plugin gave the viewer, if the plugin may open it, as its `openUrl` would.
    pub fn open_url(&self, url: &str) -> Result<(), String> {
        let Some(access) = &self.shared.access else {
            return Err(format!(
                "{} is turned off or not installed",
                self.shared.config.id
            ));
        };
        if !wings_plugins::fetch::may_open_url(&access.plugin, url) {
            return Err(format!("{} may not open {url}", access.plugin.manifest.id));
        }
        access.env.open_url(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::plugin::testing::{Frame, PRELUDE, Rig, Seen, WAIT};
    const PATCH: &str = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n";

    #[test]
    fn diff_calls_require_permission_title_and_valid_bounded_patch() {
        let denied = Rig::new(json!({}), vec![]);
        assert_eq!(
            denied.refused("await wings.openDiff({ title: 'Test' });"),
            "probe must declare \"diff\" in contributes.ui"
        );
        let rig = Rig::new(json!({ "contributes": { "ui": ["diff"] } }), vec![]);
        assert_eq!(
            rig.refused("await wings.openDiff({});"),
            "openDiff needs a title"
        );
        assert!(
            rig.refused("await wings.openDiff({ title:'x', patch:'bad' });")
                .contains("patch")
        );
        assert!(
            rig.refused("await wings.openDiff({ title:'x', patch:'x'.repeat(8*1024*1024+1) });")
                .contains("8")
        );
        assert_eq!(
            rig.refused("await wings.openDiff({ title:'x', comments:'review' });"),
            "comments must be a list"
        );
    }

    /// The desktop's `sendReply`: a reply typed in the viewer reaches the main frame's `onReply` as `{ diffId, replyTo,
    /// body }`, and the viewer hears when it's posted, or the error the handler threw.
    #[test]
    fn replies_reach_the_main_handler_and_its_errors_come_back() {
        let rig = Rig::new(json!({ "contributes": { "ui": ["diff"] } }), vec![]);
        let reporter = Frame::start(&rig.host, &format!("{PRELUDE} wings.onBroadcast(show);"));
        rig.host.start_main("wings.onReply(async (reply) => { if (reply.body === 'fail') throw new Error('GitHub refused it'); wings.broadcast({ reply }); }); wings.openDiff({title:'PR', comments:[{ id: 7, replyTo: null, path: 'a', line: 1, canReply: true }]}).then(value=>wings.broadcast(value));", |_| {}).unwrap();
        let id = reporter.report("viewer", |r| r["id"].is_string())["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let Seen::DiffOpen(view) = &rig.env.seen()[0] else {
            panic!("missing viewer")
        };
        assert_eq!(view.update.comments.as_ref().unwrap()[0].id, 7);
        let reply = |body: &str| {
            let (tx, rx) = std::sync::mpsc::channel();
            rig.host.send_reply(&id, 7, body, move |result| {
                let _ = tx.send(result);
            });
            rx.recv_timeout(WAIT).unwrap()
        };
        assert_eq!(reply("Thanks, fixed."), Ok(()));
        reporter.report("reply heard", |r| {
            r["reply"] == json!({ "diffId": id, "replyTo": 7, "body": "Thanks, fixed." })
        });
        assert_eq!(reply("fail"), Err("GitHub refused it".into()));
        // A plugin without `onReply` says so.
        rig.host.start_main("", |_| {}).unwrap();
        assert_eq!(
            reply("hello"),
            Err("This plugin doesn't take replies".into())
        );
    }

    #[test]
    fn loading_diff_updates_only_its_plugin_and_closed_updates_are_ignored() {
        let rig = Rig::new(json!({ "contributes": { "ui": ["diff"] } }), vec![]);
        let opener = Frame::start(
            &rig.host,
            &format!(
                "{PRELUDE} attempt(async()=>await wings.openDiff({{title:'Changes', refresh:true}}));"
            ),
        );
        let id = opener.report("open viewer", |r| r["ok"]["id"].is_string())["ok"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(id.starts_with("probe:"));
        rig.allowed(&format!(
            "await wings.updateDiff({}, {{patch:{}}}); return 1;",
            json!(id),
            json!(PATCH)
        ));
        let log = rig.env.seen();
        let Seen::DiffOpen(view) = &log[0] else {
            panic!("missing viewer")
        };
        assert!(view.update.patch.is_none() && view.update.error.is_none() && view.refresh);
        let Seen::DiffUpdate(_, _, update) = &log[1] else {
            panic!("missing update")
        };
        assert_eq!(update.patch.as_ref().unwrap().files.len(), 1);
        rig.allowed("await wings.updateDiff('other:1', {error:'wrong plugin'}); return 1;");
        assert_eq!(rig.env.seen().len(), 2);
        rig.host.diff_closed(&id);
        rig.allowed(&format!(
            "await wings.updateDiff({}, {{error:'late'}}); return 1;",
            json!(id)
        ));
        assert_eq!(rig.env.seen().len(), 2);
        drop(opener);
    }

    #[test]
    fn refresh_waits_for_the_main_handler_and_closing_notifies_it() {
        let rig = Rig::new(json!({ "contributes": { "ui": ["diff"] } }), vec![]);
        let reporter = Frame::start(&rig.host, &format!("{PRELUDE} wings.onBroadcast(show);"));
        rig.host.start_main("wings.onDiffRefresh(async ({diffId})=>{ await wings.updateDiff(diffId,{error:'reloaded'}); }); wings.onDiffClosed(value=>wings.broadcast({closed:value.diffId})); wings.openDiff({title:'Live',refresh:true}).then(value=>wings.broadcast(value));", |_| {}).unwrap();
        let id = reporter.report("main opens diff", |r| r["id"].is_string())["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let (tx, rx) = std::sync::mpsc::channel();
        rig.host.send_diff_refresh(&id, move |result| {
            let _ = tx.send(result);
        });
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), Ok(()));
        assert!(rig.env.seen().iter().any(|seen| matches!(seen, Seen::DiffUpdate(_, _, update) if update.error.as_deref() == Some("reloaded"))));
        rig.host.diff_closed(&id);
        reporter.report("close event", |r| r["closed"] == id);
    }

    #[test]
    fn disposing_an_opener_or_replacing_main_closes_its_viewers() {
        let rig = Rig::new(json!({ "contributes": { "ui": ["diff"] } }), vec![]);
        let opener = Frame::start(
            &rig.host,
            &format!("{PRELUDE} attempt(async()=>await wings.openDiff({{title:'Page'}}));"),
        );
        let id = opener.report("page diff", |r| r["ok"]["id"].is_string())["ok"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        drop(opener);
        assert!(
            rig.env
                .seen()
                .contains(&Seen::DiffClose("probe".into(), id))
        );
        let reporter = Frame::start(&rig.host, &format!("{PRELUDE} wings.onBroadcast(show);"));
        rig.host
            .start_main(
                "wings.openDiff({title:'Main'}).then(value=>wings.broadcast(value));",
                |_| {},
            )
            .unwrap();
        let id = reporter.report("main diff", |r| r["id"].is_string())["id"]
            .as_str()
            .unwrap()
            .to_owned();
        rig.host.start_main("", |_| {}).unwrap();
        assert!(
            rig.env
                .seen()
                .contains(&Seen::DiffClose("probe".into(), id))
        );
    }

    #[test]
    fn fatal_frame_failure_closes_the_viewer_and_refresh_errors_reach_the_host() {
        let rig = Rig::new(json!({ "contributes": { "ui": ["diff"] } }), vec![]);
        let reporter = Frame::start(&rig.host, &format!("{PRELUDE} wings.onBroadcast(show);"));
        rig.host.start_main("wings.onDiffRefresh(()=>{throw new Error('Refresh failed');}); wings.openDiff({title:'Failure'}).then(value=>wings.broadcast(value));", |_| {}).unwrap();
        let id = reporter.report("viewer", |r| r["id"].is_string())["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let (tx, rx) = std::sync::mpsc::channel();
        rig.host.send_diff_refresh(&id, move |result| {
            let _ = tx.send(result);
        });
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), Err("Refresh failed".into()));
        rig.host.start_main("wings.openDiff({title:'Fatal'}).then(value=>{wings.broadcast(value);setTimeout(()=>{while(true){}},0);});", |_| {}).unwrap();
        let id = reporter.report("fatal viewer", |r| r["id"].is_string())["id"]
            .as_str()
            .unwrap()
            .to_owned();
        rig.env.wait("fatal frame closes diff", |log| {
            log.contains(&Seen::DiffClose("probe".into(), id.clone()))
        });
        assert!(rig.host.shared.diffs.lock().unwrap().is_empty());
    }

    #[test]
    fn real_live_diff_helper_loads_refreshes_and_stops_after_close() {
        let sdk = [
            include_str!("../../../../../packages/plugin-sdk/poll.js"),
            include_str!("../../../../../packages/plugin-sdk/async.js"),
        ]
        .into_iter()
        .flat_map(|source| source.lines())
        .filter(|line| !line.starts_with("import "))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("export ", "");
        let rig = Rig::new(json!({ "contributes": { "ui": ["diff"] } }), vec![]);
        let reporter = Frame::start(&rig.host, &format!("{PRELUDE} wings.onBroadcast(show);"));
        rig.host.start_main(&format!("{sdk}\nlet loads=0; wings.onBroadcast(async()=>{{await refreshLiveDiffs();wings.broadcast({{loads}});}}); openLiveDiff({{title:'Live'}},async()=>{{loads++;return {{patch:{}}};}}).then(value=>wings.broadcast(value));",json!(PATCH)), |_| {}).unwrap();
        let id = reporter.report("SDK opened diff", |r| r["id"].is_string())["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let (tx, rx) = std::sync::mpsc::channel();
        rig.host.send_diff_refresh(&id, move |result| {
            let _ = tx.send(result);
        });
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), Ok(()));
        rig.host.diff_closed(&id);
        let barrier = Frame::start(&rig.host, "wings.broadcast({});");
        reporter.report("closed SDK viewer stopped loading", |r| r["loads"] == 2);
        let updates = rig
            .env
            .seen()
            .iter()
            .filter(|seen| matches!(seen, Seen::DiffUpdate(..)))
            .count();
        // The SDK skips an unchanged refresh update, but still invokes its loader.
        assert_eq!(updates, 1);
        drop(barrier);
    }
}
