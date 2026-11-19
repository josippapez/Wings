//! The usage and context dialogs over the chat, which `/usage`, `/cost` and `/context` and a click on the side panel's
//! rows open, as Claude Code's own screens for them do: the plan's limits with when each resets, the credits, what fills
//! the limits, and this session's totals; or how the context window is spent.

use wings_plugin::{
    format::{clock, grouped, parse_time, tokens},
    local_time::local,
    ui::{Node, button, card, dialog as panel, gauge, heading, loading, muted, notice, row, stack, text},
};

use crate::{
    chat::{self, Chat},
    usage::{ContextDetail, Credits, Kind, Panel, Share, Totals, Usage, Window},
};

const WIDTH: u32 = 640;
/// The most rows a table of contributors lists.
const ROWS: usize = 8;
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// The dialog that is open, over the conversation.
pub fn dialog(c: &Chat) -> Option<Node> {
    let (title, content) = match (c.panel?, &c.session.view_error) {
        (Panel::Usage, Some(why)) => ("Usage", notice("danger", why.as_str())),
        (Panel::Context, Some(why)) => ("Context", notice("danger", why.as_str())),
        (Panel::Usage, None) => ("Usage", c.session.usage_view.as_ref().map_or_else(loading, |usage| usage_body(usage, c.panel_week))),
        (Panel::Context, None) => ("Context", c.session.context_view.as_ref().map_or_else(loading, context_body)),
    };
    Some(panel(title, WIDTH, chat::close_panel).child(content))
}

fn section(title: &str, rows: impl IntoIterator<Item = Node>) -> Node {
    stack().prop("gap", "s").child(heading(3, title)).children(rows)
}

/// A label on the left and its value, right-aligned.
fn line(label: impl Into<String>, value: impl Into<String>) -> Node {
    row().prop("gap", "s").prop("align", "center").child(stack().prop("grow", true).child(text(label))).child(figure(value).prop("tone", "muted"))
}

// Not in the mono font: its slashed zeros read as a different number in a column of percents.
fn figure(value: impl Into<String>) -> Node {
    text(value)
}

/// A fact about the session: its muted label on the left, the value in full tone on the right.
fn fact(label: &str, value: impl Into<String>) -> Node {
    row().prop("gap", "s").prop("align", "center").child(stack().prop("grow", true).child(muted(label))).child(figure(value))
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
    // Each limit is one block of label, bar and reset time, so the gap between blocks is wider than any gap inside one.
    section(&title, [stack().prop("gap", "l").children(usage.windows.iter().map(window))])
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
    // The section is headed "Usage credits", so what's in it doesn't say so again.
    let body = match (credits.used, credits.limit) {
        (Some(used), Some(limit)) => gauge("Used", used as f64, (80, 95), Some(format!("{symbol}{:.2} of {symbol}{limit:.2}", credits.spent))),
        (Some(used), None) => gauge("Used", used as f64, (80, 95), Some(format!("{symbol}{:.2}", credits.spent))),
        (None, _) => line("Spent", format!("{symbol}{:.2}", credits.spent)),
    };
    let off = (!credits.on).then(|| muted("Off for now: they can't cover more messages.").prop("size", "small"));
    section("Usage credits", [body].into_iter().chain(off))
}

fn contributors(share: &Share, week: bool) -> Node {
    let pick = |label: &'static str, on: bool, week: bool| {
        button(label, if on { "secondary" } else { "ghost" }, false, move || chat::set_panel_week(week)).prop("size", "sm")
    };
    let switch = row().prop("gap", "xs").child(pick("Last 24h", !week, false)).child(pick("Last 7 days", week, true));
    // The note and the switch share a line, so the toggle does not take a row of its own.
    let header = row()
        .prop("gap", "s")
        .prop("align", "center")
        .prop("wrap", true)
        .child(stack().prop("grow", true).child(muted("Approximate share of usage · sessions on this computer only")))
        .child(switch);
    let mut rows = vec![header];
    if share.is_empty() {
        rows.push(muted("Nothing to show for this period."));
    }
    // What Claude Code suggests, as callouts apart from the data: the advice on top, what it is about under it.
    if !share.tips.is_empty() {
        rows.push(stack().prop("gap", "xs").children(share.tips.iter().map(|(pct, title, detail)| tip(*pct, title, detail))));
    }
    for (name, items) in [("Skills", &share.skills), ("Plugins", &share.plugins), ("MCP servers", &share.mcp_servers), ("Agents", &share.agents)] {
        if items.is_empty() {
            continue;
        }
        // A small muted header over its items, which sit indented under it.
        let list = stack().prop("gap", "xs").children(items.iter().take(ROWS).map(|(item, pct)| line(item.as_str(), format!("{pct}%"))));
        rows.push(
            stack()
                .prop("gap", "xs")
                .child(text(name).prop("tone", "muted").prop("size", "small").prop("weight", "medium"))
                .child(row().prop("gap", "s").child(stack().prop("width", 12)).child(stack().prop("grow", true).child(list))),
        );
    }
    section("What's contributing to your usage?", [stack().prop("gap", "m").children(rows)])
}

/// One of Claude Code's suggestions in a card: the advice and the share of usage it touches, and what to do under them.
fn tip(pct: u64, title: &str, detail: &str) -> Node {
    let head = row().prop("gap", "s").prop("align", "center").child(stack().prop("grow", true).child(text(title).prop("weight", "medium"))).child(figure(format!("{pct}%")).prop("tone", "muted"));
    let body = stack().prop("gap", "xs").child(head);
    card().child(if detail.is_empty() { body } else { body.child(muted(detail).prop("size", "small")) })
}

fn session(totals: &Totals) -> Node {
    let tokens = grouped;
    // Cost and time apart from the tokens, as two short blocks rather than one run of eight rows.
    let spent = stack().prop("gap", "xs").children([
        fact("Estimated cost", format!("${:.2}", totals.cost)),
        fact("Total duration (API)", duration(totals.api_ms)),
        fact("Total duration (wall)", duration(totals.wall_ms)),
        fact("Total code changes", format!("{} added, {} removed", totals.added, totals.removed)),
    ]);
    let used = stack().prop("gap", "xs").children([
        fact("Input tokens", tokens(totals.input)),
        fact("Output tokens", tokens(totals.output)),
        fact("Cache read tokens", tokens(totals.cache_read)),
        fact("Cache write tokens", tokens(totals.cache_write)),
    ]);
    section("Session", [stack().prop("gap", "m").child(spent).child(used)])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_shows_only_while_a_panel_is_open() {
        assert!(chat::with(|c| dialog(c)).is_none());
        for panel in [Panel::Usage, Panel::Context] {
            chat::with(|c| c.panel = Some(panel));
            assert!(chat::with(|c| dialog(c)).is_some());
            chat::close_panel();
            assert!(chat::with(|c| dialog(c)).is_none());
        }
    }

    #[test]
    fn a_duration_reads_in_its_largest_units() {
        assert_eq!(duration(840), "840ms");
        assert_eq!(duration(12_500), "12.5s");
        assert_eq!(duration(252_000), "4m 12s");
        assert_eq!(duration(3_900_000), "1h 5m");
    }
}
