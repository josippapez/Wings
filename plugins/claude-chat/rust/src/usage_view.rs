//! The usage and context views, which `/usage`, `/cost` and `/context` open in the chat column in place of the
//! conversation, as Claude Code's own screens for them do: the plan's limits with when each resets, the credits, what
//! fills the limits, and this session's totals; or how the context window is spent.

use wings_plugin::{
    format::{clock, grouped, parse_time, tokens},
    local_time::local,
    ui::{Node, button, gauge, heading, loading, muted, notice, row, stack, text},
};

use crate::{
    chat::{self, Chat},
    usage::{ContextDetail, Credits, Kind, Panel, Share, Totals, Usage, Window},
};

/// The same column the conversation has.
const COLUMN: u32 = 760;
/// The most rows a table of contributors lists.
const ROWS: usize = 8;
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// The header's way back to the conversation: `← Chat` and the view's name.
pub fn crumbs(c: &Chat) -> Option<Node> {
    let name = match c.panel? {
        Panel::Usage => "Usage",
        Panel::Context => "Context",
    };
    // Small, as the header it sits in is no taller than its icon buttons.
    Some(row().prop("gap", "s").prop("align", "center").child(button("← Chat", "ghost", false, chat::close_panel).prop("size", "sm")).child(muted(name)))
}

/// The view that is open.
pub fn body(c: &Chat, panel: Panel) -> Node {
    let content = match (&c.session.view_error, panel) {
        (Some(why), _) => notice("danger", why.as_str()),
        (None, Panel::Usage) => c.session.usage_view.as_ref().map_or_else(loading, |usage| usage_body(usage, c.panel_week)),
        (None, Panel::Context) => c.session.context_view.as_ref().map_or_else(loading, context_body),
    };
    Node::new("Scroll").child(stack().prop("gap", "l").prop("maxWidth", COLUMN).child(content))
}

fn section(title: &str, rows: impl IntoIterator<Item = Node>) -> Node {
    stack().prop("gap", "s").child(heading(3, title)).children(rows)
}

/// A label on the left and its value, right-aligned.
fn line(label: impl Into<String>, value: impl Into<String>) -> Node {
    row().prop("gap", "s").prop("align", "center").child(stack().prop("grow", true).child(text(label))).child(text(value).prop("tone", "muted"))
}

fn usage_body(usage: &Usage, week: bool) -> Node {
    let mut parts = vec![limits(usage)];
    parts.extend(usage.credits.as_ref().map(credits));
    let share = if week { &usage.week } else { &usage.day };
    parts.extend(share.as_ref().map(|share| contributors(share, week)));
    parts.push(session(&usage.totals));
    stack().prop("gap", "l").children(parts)
}

fn limits(usage: &Usage) -> Node {
    let title = match &usage.plan {
        Some(plan) => format!("Plan usage limits · {} plan", capital(plan)),
        None => "Plan usage limits".to_owned(),
    };
    if usage.windows.is_empty() {
        return section(&title, [muted("Your plan has no usage limits to show here. Claude Code reports them for a claude.ai account.")]);
    }
    section(&title, usage.windows.iter().map(window))
}

fn window(window: &Window) -> Node {
    gauge(window.name.as_str(), window.used as f64, (80, 95), resets(&window.resets, window.name == "Current session"))
}

/// "Resets 16:59" for the 5-hour window, "Resets Nov 22 at 16:59" for the others.
fn resets(at: &str, today: bool) -> Option<String> {
    let ms = parse_time(at)?;
    let when = local(ms as i64);
    Some(if today { format!("Resets {}", clock(ms)) } else { format!("Resets {} {} at {}", MONTHS[(when.month as usize + 11) % 12], when.day, clock(ms)) })
}

fn credits(credits: &Credits) -> Node {
    let symbol = match credits.currency.as_str() {
        "USD" => "$".to_owned(),
        "EUR" => "€".to_owned(),
        "GBP" => "£".to_owned(),
        other => format!("{other} "),
    };
    let spent = match credits.limit {
        Some(limit) => format!("{symbol}{:.2} / {symbol}{limit:.2} spent", credits.spent),
        None => format!("{symbol}{:.2} spent", credits.spent),
    };
    let body = match credits.used {
        Some(used) => gauge("Usage credits", used as f64, (80, 95), Some(spent)),
        None => line("Usage credits", spent),
    };
    section("Usage credits", [body])
}

fn contributors(share: &Share, week: bool) -> Node {
    let pick = |label: &'static str, on: bool, week: bool| {
        button(label, if on { "secondary" } else { "ghost" }, false, move || chat::set_panel_week(week)).prop("size", "sm")
    };
    let switch = row().prop("gap", "xs").child(pick("Last 24h", !week, false)).child(pick("Last 7 days", week, true));
    let mut rows = vec![muted("Approximate · sessions on this computer only"), switch];
    if share.is_empty() {
        rows.push(muted("Nothing to show for this period."));
    }
    // What Claude Code suggests, each with the share of usage it touches.
    rows.extend(share.tips.iter().map(|(pct, title, detail)| {
        let said = if detail.is_empty() { title.clone() } else { format!("{title} · {detail}") };
        line(said, format!("{pct}%"))
    }));
    for (name, items) in [("Skills", &share.skills), ("Plugins", &share.plugins), ("MCP servers", &share.mcp_servers), ("Agents", &share.agents)] {
        if items.is_empty() {
            continue;
        }
        rows.push(row().prop("gap", "s").child(stack().prop("grow", true).child(text(name).prop("weight", "medium"))).child(muted("% of usage")));
        rows.extend(items.iter().take(ROWS).map(|(item, pct)| line(item.as_str(), format!("{pct}%"))));
    }
    section("What's contributing to your usage?", rows)
}

fn session(totals: &Totals) -> Node {
    let tokens = grouped;
    section(
        "Session",
        [
            line("Estimated cost", format!("${:.2}", totals.cost)),
            line("Total duration (API)", duration(totals.api_ms)),
            line("Total duration (wall)", duration(totals.wall_ms)),
            line("Total code changes", format!("{} added, {} removed", totals.added, totals.removed)),
            line("Input tokens", tokens(totals.input)),
            line("Output tokens", tokens(totals.output)),
            line("Cache read tokens", tokens(totals.cache_read)),
            line("Cache write tokens", tokens(totals.cache_write)),
        ],
    )
}

/// "840ms", "12.5s", "4m 12s", "1h 5m".
fn duration(ms: u64) -> String {
    match ms {
        0..1000 => format!("{ms}ms"),
        1000..60_000 => format!("{:.1}s", ms as f64 / 1000.0),
        60_000..3_600_000 => format!("{}m {}s", ms / 60_000, ms / 1000 % 60),
        _ => format!("{}h {}m", ms / 3_600_000, ms / 60_000 % 60),
    }
}

fn context_body(context: &ContextDetail) -> Node {
    let used = context.used * 100 / context.most.max(1);
    let model = if context.model.is_empty() { String::new() } else { format!(" · {}", context.model) };
    let total = gauge("Context window", used as f64, (70, 90), Some(format!("{} of {} tokens{model}", grouped(context.used), grouped(context.most))));
    // A thin bar each, as the share of the whole window. The reserve and the free space are not "used", so they stay calm.
    let rows = context.categories.iter().map(|(name, count, kind)| {
        let share = *count as f64 * 100.0 / context.most.max(1) as f64;
        let levels = if *kind == Kind::Used { (70, 90) } else { (101, 101) };
        gauge(name.as_str(), share, levels, Some(format!("{} tokens", tokens(*count as f64))))
    });
    stack().prop("gap", "l").child(section("Context usage", [total])).child(section("Categories", rows))
}

fn capital(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}
