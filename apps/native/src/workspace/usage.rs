//! The usage limits Wings reads itself, as Claude Code's `/usage` does (`wings_connect::usage`): every 5 seconds while
//! Wings is in front, for each Claude account with a session on screen or that reported to the status line. What comes
//! back joins the status line's limits, so `wings.statusline()` and the HUD read them as before. Only the app that
//! serves the socket does it, never tests or screenshots.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Instant,
};

use gpui::Context;
use wings_agents::detect::AgentKind;
use wings_connect::usage::{self, Account, Fetched, Usage};
use wings_spaces::claude_accounts;

use super::{Workspace, claude::login_of};

impl Workspace {
    pub(super) fn start_usage(&self, usage: Usage, cx: &mut Context<Self>) {
        let usage = Arc::new(usage);
        cx.spawn(async move |this, cx| {
            while this
                .update(cx, |this, cx| this.ask_usage(&usage, cx))
                .is_ok()
            {
                cx.background_executor().timer(usage::EVERY).await;
            }
        })
        .detach();
    }

    /// Asks for the usage of the accounts whose turn it is, each on a thread of its own.
    fn ask_usage(&self, usage: &Arc<Usage>, cx: &mut Context<Self>) {
        let mut sessions: BTreeMap<Option<String>, BTreeSet<String>> = self
            .plugin_env
            .statusline
            .lock()
            .unwrap()
            .sessions_by_account();
        if let Some(default) = self.config.agents.as_deref() {
            for agent in self
                .agents
                .values()
                .filter(|agent| agent.kind == AgentKind::Claude)
            {
                let (Some(dir), Some(id)) = (agent.claude_dir.as_deref(), &agent.session_id) else {
                    continue;
                };
                let account = match claude_accounts::label(dir, default) {
                    Some(name) => Some(name.to_string()),
                    None if dir == default => None,
                    None => continue,
                };
                sessions.entry(account).or_default().insert(id.clone());
            }
        }
        let accounts: Vec<Account> = sessions
            .into_iter()
            .map(|(name, sessions)| Account {
                login: login_of(name.as_deref()),
                name,
                sessions,
            })
            .collect();
        for account in usage.due(&accounts, self.window_active, Instant::now()) {
            let (usage, env) = (usage.clone(), self.plugin_env.clone());
            cx.background_executor()
                .spawn(async move {
                    let fetched = usage.fetch(account.as_deref());
                    if let Fetched::Limits {
                        five_hour,
                        seven_day,
                    } = &fetched
                    {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |d| d.as_millis() as u64);
                        let login = login_of(account.as_deref());
                        env.statusline.lock().unwrap().record_usage(
                            account.as_deref(),
                            login.as_deref(),
                            five_hour.clone(),
                            seven_day.clone(),
                            now,
                        );
                    }
                    usage.done(&account, &fetched, Instant::now());
                })
                .detach();
        }
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

    use serde_json::{Value, json};
    use wings_connect::usage::{Api, Logins, NoToken, Token};

    use super::*;

    const DEFAULT_SESSION: &str = "3c21b0c5-8113-4f55-9493-fa156c3fa369";
    const OTHER_SESSION: &str = "4c21b0c5-8113-4f55-9493-fa156c3fa369";
    const OTHER: &str = "wings-usage-test-account";

    /// The default account is signed in, and the other one's login has expired. Never the keychain. The other account
    /// has no folder, so it has no login to share the default account's limits with.
    struct TestLogins;

    impl Logins for TestLogins {
        fn token(&self, account: Option<&str>) -> Result<Token, NoToken> {
            match account {
                None => Ok(Token::new("test-token".into())),
                Some(_) => Err(NoToken::Expired),
            }
        }
    }

    /// Never the network: counts the requests and answers with more used than the status line said.
    struct TestApi(Arc<AtomicUsize>);

    impl Api for TestApi {
        fn usage(&self, _: &Token) -> Option<(u16, String)> {
            self.0.fetch_add(1, SeqCst);
            let answer = r#"{ "five_hour": { "utilization": 64.0, "resets_at": "2025-02-01T16:00:00.2+00:00" }, "seven_day": null }"#;
            Some((200, answer.into()))
        }
    }

    fn status(id: &str, used: f64) -> Value {
        json!({ "session_id": id, "rate_limits": { "five_hour": { "used_percentage": used, "resets_at": 1738425600 } } })
    }

    #[test]
    fn reads_the_limits_of_each_account_that_reported_into_the_status_line_while_wings_is_in_front()
    {
        let (app, mut window) = crate::workspace::tests::open_config(|_| {});
        let asked = Arc::new(AtomicUsize::new(0));
        let usage = Arc::new(Usage::new(TestLogins, TestApi(asked.clone())));
        window.update(|w, _, cx| {
            let mut statusline = w.plugin_env.statusline.lock().unwrap();
            statusline.record(&status(DEFAULT_SESSION, 40.0), &Value::Null, None, 1);
            statusline.record(&status(OTHER_SESSION, 10.0), &json!(OTHER), None, 1);
            drop(statusline);
            w.window_active = false;
            w.ask_usage(&usage, cx);
        });
        app.run_until_parked();
        assert_eq!(
            asked.load(SeqCst),
            0,
            "nothing is asked while Wings is behind other apps"
        );
        window.update(|w, _, cx| {
            w.window_active = true;
            w.ask_usage(&usage, cx);
        });
        app.run_until_parked();
        assert_eq!(
            asked.load(SeqCst),
            1,
            "the other account has no token to ask with"
        );
        let view = window.read(|w, _| w.plugin_env.statusline.lock().unwrap().view());
        let five_hour = |account: Value| {
            view["rateLimits"]
                .as_array()
                .unwrap()
                .iter()
                .find(|l| l["account"] == account)
                .map(|l| l["fiveHour"].clone())
        };
        assert_eq!(
            five_hour(Value::Null),
            Some(json!({ "usedPercentage": 64.0, "resetsAt": 1738425600 }))
        );
        assert_eq!(
            five_hour(json!(OTHER)),
            Some(json!({ "usedPercentage": 10.0, "resetsAt": 1738425600 }))
        );
    }
}
