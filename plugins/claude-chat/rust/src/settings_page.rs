//! The Claude chat page in Wings Settings: the mode a new chat starts in, the view mode chats show and which kinds of fold
//! Verbose opens, how big its text is, and what lies behind it. It is laid out as Wings' own Settings panes are: sections of
//! rows with the name and what it does on the left and the control at the end, and a small preview under a row where a
//! choice shows.

use std::cell::RefCell;
use wings_plugin::{host::AccountChoice, task::spawn, ui::{Entry, Node, card, checkbox, chips as choose, entry, muted, row, rows_card, section, select, stack, text}};

/// How wide the background's menu is at the end of its row.
const MENU_WIDTH: u32 = 200;

use crate::{
    claude_version,
    folds::{KINDS, Kind},
    prefs,
    session::MODES,
    settings_preview::{diff, limit, places, sample, turn},
    view_mode::{self, Mode},
};

thread_local! {
    /// The Claude Code accounts, as Wings lists them, once read. The page shows a row of limits for each when there are two
    /// or more.
    static ACCOUNTS: RefCell<Vec<AccountChoice>> = const { RefCell::new(Vec::new()) };
}

/// What an account is called in a row: its name, or "Default account" for the default one, with the email when it has one.
fn account_label(account: &AccountChoice) -> String {
    let name = account.name.clone().unwrap_or_else(|| "Default account".to_owned());
    match &account.email {
        Some(email) => format!("{name} · {email}"),
        None => name,
    }
}

/// Picks `limit` for one account: "Same as above" for the shared share, or one of the shares.
fn account_share(account: &AccountChoice, limit: prefs::Limit, name: &str, shared: &str) -> Node {
    let mut options = vec![(String::new(), format!("Same as above ({shared})"))];
    options.push((prefs::OFF.to_owned(), "Off".to_owned()));
    options.extend(prefs::LIMIT_SHARES.iter().map(|(value, label)| ((*value).to_owned(), (*label).to_owned())));
    let own = prefs::own_share(&prefs::account_at().get(), account.name.as_deref(), limit).unwrap_or_default();
    let who = account.name.clone();
    let menu = select(&format!("{name} {}", if limit == prefs::Limit::Pause { "pause at" } else { "stop at" }), &options, &own, move |share| {
        prefs::set_own_share(who.as_deref(), limit, (!share.is_empty()).then(|| share.to_owned()));
    });
    stack().prop("width", MENU_WIDTH).child(menu)
}

/// What "Same as above" follows: the shared percent, or "off" when the shared checkbox is off.
fn shared_label(on: bool, share: &str) -> String {
    if on { format!("{share}%") } else { "off".to_owned() }
}

/// With two or more accounts, a row for each to pick its own pause and stop share, else the shared one above.
fn per_account(accounts: &[AccountChoice]) -> Option<Node> {
    if accounts.len() < 2 {
        return None;
    }
    let (pause, stop) = (shared_label(prefs::pause_near_limit().get(), &prefs::pause_at().get()), shared_label(prefs::stop_near_limit().get(), &prefs::stop_at().get()));
    let rows = accounts.iter().map(|account| {
        let name = account_label(account);
        let controls = row()
            .prop("gap", "s")
            .prop("wrap", true)
            .child(muted("Pause").prop("size", "small"))
            .child(account_share(account, prefs::Limit::Pause, &name, &pause))
            .child(muted("Stop").prop("size", "small"))
            .child(account_share(account, prefs::Limit::Stop, &name, &stop));
        entry(Entry { lead: None, title: text(&name).prop("weight", "medium"), detail: Some(controls), trailing: None }).key(format!("account-{}", account.name.as_deref().unwrap_or("default")))
    });
    Some(together([about("Per account", "Each account's own share, or off, where it differs from the ones above. Same as above follows them."), stack().children(rows)]))
}

/// A choice between a few, on the segmented track other Wings settings use.
fn chips(label: &str, options: &[(&str, &str)], value: &str, on_change: impl Fn(&str) + 'static) -> Node {
    choose(label, options, value, on_change).prop("segmented", true)
}

/// A setting as a row: its name and what it does on the left, its control at the end.
fn setting(name: &str, detail: &str, control: Node) -> Node {
    entry(Entry { lead: None, title: text(name).prop("weight", "medium"), detail: Some(muted(detail).prop("size", "small")), trailing: Some(control) })
}

/// A row of a name and what it does, for the choices drawn under it.
fn about(name: &str, detail: &str) -> Node {
    entry(Entry { lead: None, title: text(name).prop("weight", "medium"), detail: Some(muted(detail).prop("size", "small")), trailing: None })
}

/// A setting that is a tick: the tick's label is its name.
fn toggle(tick: Node, detail: &str) -> Node {
    entry(Entry { lead: None, title: tick, detail: Some(muted(detail).prop("size", "small")), trailing: None })
}

/// A section: its rows in one card split by hairlines, as Wings' own Settings panes group theirs.
fn group(name: &str, rows: impl IntoIterator<Item = Node>) -> Node {
    section(name, None, [rows_card(rows)])
}

/// One row of a group made of a setting and what goes with it, like its preview.
fn together(parts: impl IntoIterator<Item = Node>) -> Node {
    stack().prop("gap", "m").children(parts)
}

/// What a choice does, in a card under its row, in the rows' inset.
fn preview(content: Node) -> Node {
    entry(Entry { lead: None, title: card().child(content), detail: None, trailing: None })
}

fn view() -> Vec<Node> {
    let modes = [Mode::Normal, Mode::Thinking, Mode::Verbose].map(|mode| (mode.name(), mode.name()));
    let mode = Mode::of(prefs::detailed().get(), prefs::thinking().get());
    let checks: Vec<Node> = KINDS
        .iter()
        .map(|(kind, key, label)| {
            let kind = *kind;
            // The two longest names are cut to fit a column.
            let label = match kind {
                Kind::Summary => "Summaries of finished agents",
                Kind::Note => "Notes from Claude Code",
                _ => *label,
            };
            checkbox(label, prefs::opens(kind).get(), move |on| drop(prefs::opens(kind).set(on))).key(*key)
        })
        .collect();
    // Two columns of ticks, the first holding the larger half.
    let second = checks.len() / 2;
    let mut checks = checks.into_iter();
    let left: Vec<Node> = checks.by_ref().take(KINDS.len() - second).collect();
    let ticks = row()
        .prop("gap", "l")
        .prop("align", "start")
        .child(stack().prop("grow", true).prop("gap", "s").children(left))
        .child(stack().prop("grow", true).prop("gap", "s").children(checks));
    let (pause_at, stop_at) = (prefs::pause_at().get(), prefs::stop_at().get());
    let split = prefs::diff_split().get();
    vec![stack()
        .prop("gap", "l")
        .child(group(
            "New chats",
            [setting(
                "Permission mode",
                "The mode a chat starts in. The picker in a chat's message box still changes it for that chat.",
                chips("Permission mode", &MODES, &prefs::mode().get(), |mode| drop(prefs::mode().set(mode.to_owned()))),
            )],
        ))
        .child(group(
            "Output",
            [
                together([
                    setting(
                        "View mode",
                        "How much of a turn a chat shows. The View mode button in a chat and Ctrl+O change the same setting.",
                        chips("View mode", &modes, mode.name(), |name| {
                            if let Some(mode) = [Mode::Normal, Mode::Thinking, Mode::Verbose].into_iter().find(|mode| mode.name() == name) {
                                view_mode::set(mode);
                            }
                        }),
                    ),
                    preview(turn(mode, |kind| prefs::opens(kind).get())),
                ]),
                toggle(
                    checkbox("Start subagent reports folded", prefs::fold_reports().get(), |on| drop(prefs::fold_reports().set(on))).key("fold-reports"),
                    "A subagent's report to the chat shows in full, in every view mode. Tick this to fold each to its first line, which a click opens.",
                ),
                together([about("What Verbose opens", "Each kind starts open in Verbose unless you clear it. Normal keeps all of them shut."), preview(ticks)]),
            ],
        ))
        .child(group(
            "Plan limit",
            [
                together([
                    toggle(
                        checkbox("Tell Claude to pause near the limit", prefs::pause_near_limit().get(), |on| drop(prefs::pause_near_limit().set(on))).key("pause-near-limit"),
                        "When your plan's 5-hour or weekly usage passes the share you pick, Claude is told to pause and ask you before more work, in every Claude Code session in Wings. Past the limit, work is billed as extra usage.",
                    ),
                    setting("Pause at", "The share of the limit.", chips("Pause at", &prefs::LIMIT_SHARES, &pause_at, |share| drop(prefs::pause_at().set(share.to_owned())))),
                    preview(limit("Claude pauses and asks", prefs::percent(&pause_at), false)),
                ]),
                together([
                    toggle(
                        checkbox("Stop the chat's turn near the limit", prefs::stop_near_limit().get(), |on| drop(prefs::stop_near_limit().set(on))).key("stop-near-limit"),
                        "A chat can also stop Claude's turn itself, as its Stop button does, once at this share and once more when the limit is used up. Send a message to go on.",
                    ),
                    setting("Stop at", "The share of the limit.", chips("Stop at", &prefs::LIMIT_SHARES, &stop_at, |share| drop(prefs::stop_at().set(share.to_owned())))),
                    preview(limit("The chat stops Claude's turn", prefs::percent(&stop_at), true)),
                ]),
            ]
            .into_iter()
            .chain(ACCOUNTS.with(|accounts| per_account(&accounts.borrow())))
            .collect::<Vec<_>>(),
        ))
        .child(group(
            "Remote Control",
            [toggle(
                checkbox("Turn on Remote Control in every new chat", prefs::remote_on_start().get(), |on| drop(prefs::remote_on_start().set(on))).key("remote-on-start"),
                "Each new chat turns on Remote Control as it starts, so you can follow and answer it on claude.ai or in the Claude app. The button by the model menu still turns it off for that chat.",
            )],
        ))
        .child(group(
            "Appearance",
            [
                together([
                    setting(
                        "Session panel",
                        "Where a chat shows its session: changes, context and usage, tasks, subagents and terminals.",
                        chips("Session panel", &prefs::SESSION_PANELS, &prefs::session_panel().get(), |place| drop(prefs::session_panel().set(place.to_owned()))),
                    ),
                    preview(places(prefs::in_sidebar(&prefs::session_panel().get()))),
                ]),
                together([
                    setting(
                        "Diff layout",
                        "How a chat shows the lines an edit changes.",
                        chips("Diff layout", &[("one", "One column"), ("split", "Side by side")], if split { "split" } else { "one" }, |layout| drop(prefs::diff_split().set(layout == "split"))),
                    ),
                    preview(diff(split)),
                ]),
                together([
                    setting(
                        "Text size",
                        "The size of the text in the conversation, in every chat.",
                        chips("Text size", &prefs::TEXT_SIZES, &prefs::text_size().get(), |size| drop(prefs::text_size().set(size.to_owned()))),
                    ),
                    preview(sample(prefs::scale_of(&prefs::text_size().get()))),
                ]),
                setting(
                    "Background",
                    "What lies behind the conversation. Patterns and gradients are dim, move slowly while Wings is in front, and stand still when your Mac reduces motion. Some need a newer Wings.",
                    background(),
                ),
            ],
        ))
        .child(group(
            "Claude Code",
            [together([about("Version", "The version every chat runs, and whether a newer one is out. Update Claude Code runs `claude upgrade`."), preview_plain(claude_version::section())])],
        ))]
}

/// The background's choice: a menu, as seventeen don't fit on a track in a narrow page.
fn background() -> Node {
    let options: Vec<(String, String)> =
        prefs::BACKGROUNDS.iter().map(|(value, label)| ((*value).to_owned(), (*label).to_owned())).collect();
    let menu = select("Background", &options, &prefs::background().get(), |kind| drop(prefs::background().set(kind.to_owned())));
    stack().prop("width", MENU_WIDTH).child(menu)
}

/// A part of the page that isn't a preview, in the rows' inset.
fn preview_plain(content: Node) -> Node {
    entry(Entry { lead: None, title: content, detail: None, trailing: None })
}

// A test build holds the main frame's `start` too: only the page's module exports this one.
#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    claude_version::check();
    spawn(async {
        if let Ok(accounts) = wings_plugin::host::accounts().await {
            ACCOUNTS.with(|kept| *kept.borrow_mut() = accounts);
        }
    });
    wings_plugin::run(view);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_sits_on_a_track_and_a_setting_has_its_name_and_detail_before_its_control() {
        let tree = wings_plugin::ui::tree_json(chips("Mode", &[("a", "A")], "a", |_| {}));
        assert_eq!(tree["props"]["segmented"], true);
        let tree = wings_plugin::ui::tree_json(setting("Mode", "What it does", text("control")));
        assert_eq!(tree["type"], "ListRow");
        let slots: Vec<&str> = tree["children"].as_array().unwrap().iter().map(|slot| slot["props"]["name"].as_str().unwrap()).collect();
        assert_eq!(slots, ["title", "detail", "trailing"]);
    }

    #[test]
    fn each_account_gets_a_row_of_limits_only_with_two_or_more() {
        let account = |name: Option<&str>| AccountChoice { name: name.map(str::to_owned), email: None, signed_in: true, current: false };
        assert!(per_account(&[account(None)]).is_none());
        let rows = wings_plugin::ui::tree_json(per_account(&[account(None), account(Some("work"))]).unwrap()).to_string();
        assert!(rows.contains("Default account pause at") && rows.contains("work stop at"), "{rows}");
        // Pause is on by default and stop is off, and "Same as above" says what each follows.
        assert!(rows.contains("Same as above (90%)") && rows.contains("Same as above (off)"), "{rows}");
        assert!(rows.contains("\"Off\""), "{rows}");
    }

    #[test]
    fn the_background_is_a_choice_of_the_plain_ground_pixel_drift_or_a_gradient_and_starts_plain() {
        fn select_named<'a>(node: &'a serde_json::Value, label: &str) -> Option<&'a serde_json::Value> {
            if node["type"] == "Select" && node["props"]["label"] == label {
                return Some(node);
            }
            node["children"].as_array()?.iter().find_map(|child| select_named(child, label))
        }
        let page = wings_plugin::ui::tree_json(view().remove(0));
        let select = select_named(&page, "Background").expect("the background's choice");
        assert_eq!(select["props"]["value"], "plain");
        let options = select["props"]["options"].as_array().unwrap();
        let labels: Vec<&str> = options.iter().map(|option| option["label"].as_str().unwrap()).collect();
        assert_eq!(
            labels,
            ["Plain", "Pixel drift", "Pixel rain", "Checker shimmer", "Embers", "Black and white", "Ocean pixels", "Meadow pixels", "Dusk pixels", "Pixel blocks", "Ordered dither", "Grainy gradient", "Grainy lagoon", "Halo", "Pensive", "Mint", "Interstella", "Nighty night", "Topography", "Flow lines", "Grid", "Dot matrix", "Silk", "Cells"]
        );
        assert_eq!(options[2]["value"], "pixels:rain");
        assert_eq!(options[13]["value"], "gradient:halo");
    }
}
