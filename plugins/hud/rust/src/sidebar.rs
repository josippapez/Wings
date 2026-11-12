//! The HUD sidebar page: usage limits and memory, then one session's context, MCP servers and skills. The main script
//! keeps the state and tells it here.

use crate::{
    CONTEXT_CRITICAL, CONTEXT_WARN, LIMIT_CRITICAL, LIMIT_WARN, SNAPSHOT, WANT,
    format::gb,
    view::{limits_for, shown},
};
use serde_json::Value;
use std::cell::RefCell;
use wings_plugin::{
    format::tokens,
    messages,
    ui::{Node, card, disclosure, dot, empty, gauge, list, loading, muted, row, select, stack, text},
};

const LIMIT_LEVELS: (u32, u32) = (LIMIT_WARN as u32, LIMIT_CRITICAL as u32);
const CONTEXT_LEVELS: (u32, u32) = (CONTEXT_WARN as u32, CONTEXT_CRITICAL as u32);

#[derive(Default)]
struct Page {
    /// What main last told, or none before it has.
    hud: Option<Value>,
    /// The session you picked from the list, with what was followed then: it shows until you select another pane.
    picked: Option<(String, Option<String>)>,
    /// The session of the pane selected last, which the page follows.
    followed: Option<String>,
}

thread_local! {
    static PAGE: RefCell<Page> = RefCell::new(Page::default());
}

fn heading(title: impl Into<String>) -> Node {
    Node::new("Heading").text(title)
}

fn limit(label: &str, of: &str, l: &Value) -> Option<Node> {
    let note = l["resets"].as_str().map(|resets| format!("Resets in {resets}"));
    l.is_object().then(|| gauge(format!("{label}{of}"), l["percent"].as_f64().unwrap_or(0.0), LIMIT_LEVELS, note))
}

fn server_row(server: &Value) -> Node {
    let (name, state) = (server["name"].as_str().unwrap_or_default(), server["state"].as_str().unwrap_or_default());
    let tone = match state {
        "failed" => Some("danger"),
        "connecting" | "sign-in" => Some("muted"),
        _ => None,
    };
    let mark = match tone {
        Some(tone) => dot().prop("tone", tone),
        None => dot(),
    };
    let said = match state {
        "failed" => Some(text("Failed").prop("tone", "danger")),
        "connecting" => Some(muted("Connecting")),
        _ => None,
    };
    let call = if server["lastFailed"] == true {
        Some(text("Last call failed").prop("tone", "warn"))
    } else if server["used"] == true {
        Some(muted("Used"))
    } else {
        None
    };
    let error = server["error"].as_str().filter(|error| state == "failed" && !error.is_empty());
    Node::new("ListItem").key(name).child(
        stack()
            .prop("gap", "none")
            .prop("grow", true)
            .child(row().child(mark).child(text(name).prop("grow", true).prop("truncate", true)).maybe(said).maybe(call))
            .maybe(error.map(|error| text(error).prop("tone", "danger").prop("size", "tiny").prop("truncate", true))),
    )
}

fn view() -> Vec<Node> {
    PAGE.with(|page| {
        let mut page = page.borrow_mut();
        let Some(hud) = page.hud.clone() else {
            return vec![loading()];
        };
        let none = Vec::new();
        let (o, sessions) = (&hud["overview"], hud["sessions"].as_array().unwrap_or(&none));
        if let Some(focused) = sessions.iter().find(|s| s["focused"] == true).and_then(|s| s["id"].as_str()) {
            page.followed = Some(focused.to_owned());
        }
        let s = shown(sessions, page.followed.as_deref(), page.picked.as_ref());
        // The limits of the account the shown session runs as come first.
        let all = o["limits"].as_array().unwrap_or(&none);
        let (limits, missing) = limits_for(all, s.and_then(|s| s.get("account")));
        let account = |account: &Value| account.as_str().unwrap_or("default").to_owned();

        let mut usage = card();
        // Nothing says which account the session runs as, so every account's limits are named.
        let silent = s.is_some_and(|s| s["silent"] == true);
        if silent {
            usage = usage.child(text("This session's Claude account isn't connected to Wings, so its limits aren't known. Connect Claude Code in Plugins.").prop("tone", "warn"));
        }
        if let (true, Some(s)) = (missing, s) {
            usage = usage.child(muted(format!("This session runs as the {} account, which hasn't reported its limits yet.", account(&s["account"]))));
        }
        for a in limits {
            let of = if all.len() > 1 || missing || silent { format!(" · {}", account(&a["account"])) } else { String::new() };
            usage = usage.maybe(limit("5-hour limit", &of, &a["fiveHour"])).maybe(limit("Weekly limit", &of, &a["sevenDay"]));
        }
        if o["statusline"] != true {
            usage = usage.child(muted("Your 5-hour and weekly limits show after Claude Code's next reply, on Pro and Max plans. If they don't, connect Claude Code in Plugins."));
        }
        if let Some(memory) = o["memory"].as_object() {
            let (used, total) = (memory["used"].as_f64().unwrap_or(0.0), memory["total"].as_f64().unwrap_or(0.0));
            usage = usage.child(gauge("Memory", memory["percent"].as_f64().unwrap_or(0.0), LIMIT_LEVELS, Some(format!("{} of {} in use", gb(used), gb(total)))));
        }
        let mut out = vec![heading("Usage").key("usage-title"), usage.key("usage")];

        let Some(s) = s else {
            out.push(empty("No Claude session in your panes.", "Run claude in a pane to see its context, MCP servers and skills.").key("empty"));
            return out;
        };
        let id = s["id"].as_str().unwrap_or_default();
        if sessions.len() > 1 {
            let options: Vec<(String, String)> = sessions
                .iter()
                .map(|x| {
                    let id = x["id"].as_str().unwrap_or_default();
                    let name = x["name"].as_str().map_or_else(|| id.chars().take(8).collect(), str::to_owned);
                    (id.to_owned(), format!("{} · {name}", x["project"].as_str().unwrap_or_default()))
                })
                .collect();
            out.push(
                select("Session", &options, id, |picked| {
                    PAGE.with(|page| {
                        let mut page = page.borrow_mut();
                        page.picked = Some((picked.to_owned(), page.followed.clone()));
                    });
                })
                .key("session"),
            );
        }

        out.push(heading("Context").key("context-title"));
        let ctx = &s["context"];
        out.push(
            card()
                .child(if ctx.is_object() {
                    let count = |name: &str| tokens(ctx[name].as_f64().unwrap_or(0.0));
                    let cache = ctx["cache"].as_str().map(|cache| format!(" · prompt cache {}", cache.to_lowercase())).unwrap_or_default();
                    gauge("Context window", ctx["percent"].as_f64().unwrap_or(0.0), CONTEXT_LEVELS, Some(format!("{} of {} tokens{cache}", count("tokens"), count("window"))))
                } else {
                    muted("Claude hasn't replied yet.")
                })
                .key("context"),
        );

        let servers = s["servers"].as_array().unwrap_or(&none);
        let (sign_in, on): (Vec<&Value>, Vec<&Value>) = servers.iter().partition(|x| x["state"] == "sign-in");
        out.push(heading(format!("MCP servers ({})", on.len())).key("servers-title"));
        out.push(if on.is_empty() { muted("None connected yet.").key("servers") } else { list().children(on.into_iter().map(server_row)).key("servers") });
        if !sign_in.is_empty() {
            out.push(disclosure(format!("{} need sign-in", sign_in.len()), list().children(sign_in.into_iter().map(server_row))).key("sign-in"));
        }

        let skills = s["skills"].as_array().unwrap_or(&none);
        out.push(heading(format!("Skills ({})", skills.len())).key("skills-title"));
        out.push(if skills.is_empty() {
            muted("None used yet.").key("skills")
        } else {
            list()
                .children(skills.iter().filter_map(Value::as_str).map(|name| Node::new("ListItem").key(name).child(text(name).prop("grow", true).prop("truncate", true))))
                .key("skills")
        });
        out
    })
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run(view);
    messages::on(SNAPSHOT, |hud| PAGE.with(|page| page.borrow_mut().hud = Some(hud.clone())));
    messages::tell(WANT, &Value::Null);
}
