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
    use crate::page::{
        plugin::testing::{Caller, Frame, Rig, Seen, WAIT, echoes, idle},
        runtime::fake::{self, Cx, Guest},
    };
    use std::collections::HashMap;
    const PATCH: &str = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n";

    /// What a main does with what it hears about the viewer it opened.
    #[derive(Clone, Copy)]
    enum Then {
        Nothing,
        /// Posts a reply, or fails it when it says "fail".
        Replies,
        /// Reloads the viewer when it is refreshed.
        Reloads,
        /// Fails when it is refreshed.
        Throws,
        /// Is stopped a moment after it opened the viewer.
        Stops,
    }

    /// A main that opens a viewer with `params`, broadcasts what the host answered, and then does `then`.
    struct Opens {
        params: Value,
        then: Then,
        opened: u32,
        /// The refresh each `updateDiff` of a reload is for.
        reloads: HashMap<u32, u32>,
    }

    impl Guest for Opens {
        fn start(&mut self, cx: &mut Cx<'_>) {
            self.opened = cx.call("openDiff", self.params.clone());
        }

        fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, id: u32, text: &str) {
            let heard: Value = serde_json::from_str(text).unwrap_or_default();
            match (kind, self.then) {
                ("resolve", then) if id == self.opened => {
                    cx.link().broadcast(text.into()).unwrap();
                    if matches!(then, Then::Stops) {
                        cx.later(0);
                    }
                }
                ("resolve", _) => {
                    if let Some(token) = self.reloads.remove(&id) {
                        cx.call("actionDone", json!({ "token": token, "error": null }));
                    }
                }
                ("reject", _) => {
                    if let Some(token) = self.reloads.remove(&id) {
                        cx.call("actionDone", json!({ "token": token, "error": text }));
                    }
                }
                ("diffRefresh", Then::Reloads) => {
                    let call = cx.call(
                        "updateDiff",
                        json!({ "id": heard["diffId"], "error": "reloaded" }),
                    );
                    self.reloads.insert(call, id);
                }
                ("diffRefresh", Then::Throws) => {
                    cx.call(
                        "actionDone",
                        json!({ "token": id, "error": "Refresh failed" }),
                    );
                }
                ("diffClosed", Then::Reloads) => {
                    cx.link()
                        .broadcast(json!({ "closed": heard["diffId"] }).to_string())
                        .unwrap();
                }
                ("reply", Then::Replies) => {
                    let error = (heard["body"] == "fail").then_some("GitHub refused it");
                    if error.is_none() {
                        cx.link()
                            .broadcast(json!({ "reply": heard }).to_string())
                            .unwrap();
                    }
                    cx.call("actionDone", json!({ "token": id, "error": error }));
                }
                _ => {}
            }
        }

        fn timer(&mut self, cx: &mut Cx<'_>, _id: u32) {
            cx.stop("the main ran for too long and was stopped");
        }
    }

    fn opens(params: Value, then: Then) -> String {
        fake::source("main", move || {
            Box::new(Opens {
                params: params.clone(),
                then,
                opened: 0,
                reloads: HashMap::new(),
            })
        })
    }

    #[test]
    fn diff_calls_require_permission_title_and_valid_bounded_patch() {
        let denied = Rig::new(json!({}), vec![]);
        assert_eq!(
            denied.refuses("openDiff", json!({ "title": "Test" })),
            "probe must declare \"diff\" in contributes.ui"
        );
        let rig = Rig::new(json!({ "contributes": { "ui": ["diff"] } }), vec![]);
        assert_eq!(rig.refuses("openDiff", json!({})), "openDiff needs a title");
        assert!(
            rig.refuses("openDiff", json!({ "title": "x", "patch": "bad" }))
                .contains("patch")
        );
        assert!(
            rig.refuses(
                "openDiff",
                json!({ "title": "x", "patch": "x".repeat(8 * 1024 * 1024 + 1) })
            )
            .contains("8")
        );
        assert_eq!(
            rig.refuses("openDiff", json!({ "title": "x", "comments": "review" })),
            "comments must be a list"
        );
    }

    /// The desktop's `sendReply`: a reply typed in the viewer reaches the main frame's `onReply` as `{ diffId, replyTo,
    /// body }`, and the viewer hears when it's posted, or the error the handler threw.
    #[test]
    fn replies_reach_the_main_handler_and_its_errors_come_back() {
        let rig = Rig::new(json!({ "contributes": { "ui": ["diff"] } }), vec![]);
        let reporter = Frame::start(&rig.host, &echoes());
        let comments =
            json!([{ "id": 7, "replyTo": null, "path": "a", "line": 1, "canReply": true }]);
        rig.host
            .start_main(
                &opens(
                    json!({ "title": "PR", "comments": comments }),
                    Then::Replies,
                ),
                |_| {},
            )
            .unwrap();
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
        struct Declines;
        impl Guest for Declines {
            fn deliver(&mut self, cx: &mut Cx<'_>, kind: &str, id: u32, _text: &str) {
                if kind == "reply" {
                    cx.call(
                        "actionDone",
                        json!({ "token": id, "error": "This plugin doesn't take replies" }),
                    );
                }
            }
        }
        rig.host
            .start_main(&fake::source("main", || Box::new(Declines)), |_| {})
            .unwrap();
        assert_eq!(
            reply("hello"),
            Err("This plugin doesn't take replies".into())
        );
    }

    #[test]
    fn loading_diff_updates_only_its_plugin_and_closed_updates_are_ignored() {
        let rig = Rig::new(json!({ "contributes": { "ui": ["diff"] } }), vec![]);
        let opener = Caller::start(&rig.host);
        let id = opener
            .call("openDiff", json!({ "title": "Changes", "refresh": true }))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(id.starts_with("probe:"));
        rig.allows("updateDiff", json!({ "id": id, "patch": PATCH }));
        let log = rig.env.seen();
        let Seen::DiffOpen(view) = &log[0] else {
            panic!("missing viewer")
        };
        assert!(view.update.patch.is_none() && view.update.error.is_none() && view.refresh);
        let Seen::DiffUpdate(_, _, update) = &log[1] else {
            panic!("missing update")
        };
        assert_eq!(update.patch.as_ref().unwrap().files.len(), 1);
        rig.allows(
            "updateDiff",
            json!({ "id": "other:1", "error": "wrong plugin" }),
        );
        assert_eq!(rig.env.seen().len(), 2);
        rig.host.diff_closed(&id);
        rig.allows("updateDiff", json!({ "id": id, "error": "late" }));
        assert_eq!(rig.env.seen().len(), 2);
        drop(opener);
    }

    #[test]
    fn refresh_waits_for_the_main_handler_and_closing_notifies_it() {
        let rig = Rig::new(json!({ "contributes": { "ui": ["diff"] } }), vec![]);
        let reporter = Frame::start(&rig.host, &echoes());
        rig.host
            .start_main(
                &opens(json!({ "title": "Live", "refresh": true }), Then::Reloads),
                |_| {},
            )
            .unwrap();
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
        let opener = Caller::start(&rig.host);
        let id = opener.call("openDiff", json!({ "title": "Page" })).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        drop(opener);
        assert!(
            rig.env
                .seen()
                .contains(&Seen::DiffClose("probe".into(), id))
        );
        let reporter = Frame::start(&rig.host, &echoes());
        rig.host
            .start_main(&opens(json!({ "title": "Main" }), Then::Nothing), |_| {})
            .unwrap();
        let id = reporter.report("main diff", |r| r["id"].is_string())["id"]
            .as_str()
            .unwrap()
            .to_owned();
        rig.host.start_main(&idle("main"), |_| {}).unwrap();
        assert!(
            rig.env
                .seen()
                .contains(&Seen::DiffClose("probe".into(), id))
        );
    }

    #[test]
    fn fatal_frame_failure_closes_the_viewer_and_refresh_errors_reach_the_host() {
        let rig = Rig::new(json!({ "contributes": { "ui": ["diff"] } }), vec![]);
        let reporter = Frame::start(&rig.host, &echoes());
        rig.host
            .start_main(&opens(json!({ "title": "Failure" }), Then::Throws), |_| {})
            .unwrap();
        let id = reporter.report("viewer", |r| r["id"].is_string())["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let (tx, rx) = std::sync::mpsc::channel();
        rig.host.send_diff_refresh(&id, move |result| {
            let _ = tx.send(result);
        });
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), Err("Refresh failed".into()));
        rig.host
            .start_main(&opens(json!({ "title": "Fatal" }), Then::Stops), |_| {})
            .unwrap();
        let id = reporter.report("fatal viewer", |r| r["id"].is_string())["id"]
            .as_str()
            .unwrap()
            .to_owned();
        rig.env.wait("fatal frame closes diff", |log| {
            log.contains(&Seen::DiffClose("probe".into(), id.clone()))
        });
        assert!(rig.host.shared.diffs.lock().unwrap().is_empty());
    }
}
