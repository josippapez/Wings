//! The Claude chat page in Wings Settings: the mode a new chat starts in, the view mode chats show and which kinds of fold
//! Verbose opens, how big its text is, and what lies behind it. It is laid out as Wings' own Settings panes are: sections of
//! rows with the name and what it does on the left and the control at the end. Where the page is wide, a made-up chat
//! session stays in a pane on the left and shows the view options as they are set, with the choices on the right; where it
//! is narrow the session sits above them. A fold kind the session doesn't show has a small preview under its row.

use std::cell::{Cell, RefCell};
use wings_plugin::{host::AccountChoice, task::spawn, ui::{Entry, Node, card, checkbox, chips as choose, entry, muted, row, rows_card, section, select, stack, text}};

/// How wide the background's menu is at the end of its row.
const MENU_WIDTH: u32 = 200;

use crate::{
    claude_version,
    folds::{KINDS, Kind},
    prefs,
    session::MODES,
    settings_preview::{self, limit, session},
    view_mode::{self, Mode},
};

/// How wide the session pane is, and how wide the page has to be for it to sit beside the choices. Under that it moves to
/// the top of the choices, as the pane isn't drawn then.
const SESSION_WIDTH: u32 = 400;
const SIDE_BY_SIDE: u32 = 720;

thread_local! {
    /// Whether the page is narrower than `SIDE_BY_SIDE`, as the split beside the choices says.
    static NARROW: Cell<bool> = const { Cell::new(false) };
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

const STARTS_OPEN: &str = "What starts open";
const STARTS_OPEN_DETAIL: &str = "A ticked fold starts open, a cleared one starts shut, and a click on it flips it. Every row but the subagent reports applies in the Verbose view mode only: Normal keeps those shut. Subagent reports apply in every view mode.";
const REPORT_LABEL: &str = "Subagent reports";
const REPORT_DETAIL: &str = "A subagent's full report in the chat, under \"↩ Report from …\". Every view mode. Clear it to start each as its first line. This is not the summary under an \"… finished\" line.";

/// A fold kind's label and its one-line description: what it is, and when it applies.
fn kind_text(kind: Kind, label: &'static str) -> (&'static str, &'static str) {
    match kind {
        Kind::Steps => (label, "The line over a run of tool calls, like \"Ran 1 command · Read 1 file\". Open, it lists the calls. Verbose only."),
        Kind::Command => (label, "What a Bash command printed, under its call. Verbose only."),
        Kind::Read => (label, "The file a Read gave back, under its call. Verbose only."),
        Kind::Edit => (label, "An edit's diff, or the content of a written file, under its call. Verbose only."),
        Kind::Search => (label, "What a Grep or a Glob found, under its call. Verbose only."),
        Kind::Web => (label, "The page a fetch got, or what a web search found, under its call. Verbose only."),
        Kind::Subagent => (label, "A subagent call's result, under its call. Verbose only."),
        Kind::Other => (label, "What any other tool printed, like an MCP tool or a skill, under its call. Verbose only."),
        Kind::Thought => (label, "Claude's thinking, under \"Thought for 7s\". Verbose only: the Thinking view mode opens it whatever this says."),
        Kind::Summary => ("Summaries of finished agents and commands", "The short summary under an \"… finished\" line. Verbose only."),
        Kind::Note => ("Notes from Claude Code", "What Claude Code wrote in your name, like hook output or a saved memory, as a \"⚙\" line. Verbose only."),
    }
}

/// Whether the session pane shows the kind's fold.
fn in_session(kind: Kind) -> bool {
    matches!(kind, Kind::Steps | Kind::Command | Kind::Read | Kind::Edit | Kind::Thought)
}

/// The mock session, as the view options are now.
fn session_now() -> Node {
    let mode = Mode::of(prefs::detailed().get(), prefs::thinking().get());
    session(mode, |kind| prefs::opens(kind).get(), prefs::scale_of(&prefs::text_size().get()), prefs::diff_split().get(), prefs::in_sidebar(&prefs::session_panel().get()))
}

/// The session in a section of its own, as a pane beside the choices or over them.
fn session_section() -> Node {
    section("Session preview", Some("A chat with the view options as they are set."), [session_now()])
}

/// The page: the session pane beside the choices, each scrolling on its own, or in a narrow page the session over them.
fn view() -> Vec<Node> {
    let narrow = NARROW.with(Cell::get);
    let (start, end) = if narrow { (Vec::new(), vec![session_section(), choices()]) } else { (vec![session_section()], vec![choices()]) };
    let split = Node::new("Split")
        .prop("defaultSize", SESSION_WIDTH)
        .prop("collapseBelow", SIDE_BY_SIDE)
        // Room each side of the divider, so the session's cards and the choices' don't touch it.
        .prop("gap", "l")
        .on("onNarrow", |payload| NARROW.with(|narrow| narrow.set(payload["narrow"].as_bool().unwrap_or(false))))
        .child(Node::new("Slot").prop("name", "start").children(start))
        .child(Node::new("Slot").prop("name", "end").children(end));
    vec![split]
}

fn choices() -> Node {
    let modes = [Mode::Normal, Mode::Thinking, Mode::Verbose].map(|mode| (mode.name(), mode.name()));
    let mode = Mode::of(prefs::detailed().get(), prefs::thinking().get());
    // Each fold kind is a tick with what it is, and under it the chat element it opens or shuts. The subagent report's tick
    // sits next to the summaries': ticked is open, as for every row, and is the opposite of what `fold_reports` keeps.
    let mut starts: Vec<Node> = Vec::new();
    for (kind, key, label) in KINDS {
        let (label, detail) = kind_text(kind, label);
        let open = prefs::opens(kind).get();
        let tick = toggle(checkbox(label, open, move |on| drop(prefs::opens(kind).set(on))).key(key), detail);
        // The session shows these four, and the thoughts, in Verbose: a preview of each would say it twice.
        starts.push(if in_session(kind) { tick } else { together([tick, preview(settings_preview::kind(kind, open))]) });
        if kind == Kind::Summary {
            let open = !prefs::fold_reports().get();
            starts.push(together([
                toggle(checkbox(REPORT_LABEL, open, |on| drop(prefs::fold_reports().set(!on))).key("fold-reports"), REPORT_DETAIL),
                preview(settings_preview::report(open)),
            ]));
        }
    }
    let (pause_at, stop_at) = (prefs::pause_at().get(), prefs::stop_at().get());
    let split = prefs::diff_split().get();
    stack()
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
                setting(
                        "View mode",
                        "How much of a turn a chat shows. The View mode button in a chat and Ctrl+O change the same setting.",
                        chips("View mode", &modes, mode.name(), |name| {
                            if let Some(mode) = [Mode::Normal, Mode::Thinking, Mode::Verbose].into_iter().find(|mode| mode.name() == name) {
                                view_mode::set(mode);
                            }
                        }),
                    ),
                about(STARTS_OPEN, STARTS_OPEN_DETAIL),
            ]
            .into_iter()
            .chain(starts)
            .collect::<Vec<_>>(),
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
                    setting(
                        "Session panel",
                        "Where a chat shows its session: changes, context and usage, tasks, subagents and terminals.",
                        chips("Session panel", &prefs::SESSION_PANELS, &prefs::session_panel().get(), |place| drop(prefs::session_panel().set(place.to_owned()))),
                    ),
                    setting(
                        "Diff layout",
                        "How a chat shows the lines an edit changes.",
                        chips("Diff layout", &[("one", "One column"), ("split", "Side by side")], if split { "split" } else { "one" }, |layout| drop(prefs::diff_split().set(layout == "split"))),
                    ),
                    setting(
                        "Text size",
                        "The size of the text in the conversation, in every chat.",
                        chips("Text size", &prefs::TEXT_SIZES, &prefs::text_size().get(), |size| drop(prefs::text_size().set(size.to_owned()))),
                    ),
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
        ))
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
    fn the_session_sits_beside_the_choices_with_room_at_the_divider_and_over_them_when_narrow() {
        let slot = |page: &serde_json::Value, name: &str| page["children"].as_array().unwrap().iter().find(|slot| slot["props"]["name"] == name).unwrap().to_string();
        NARROW.with(|narrow| narrow.set(false));
        let wide = wings_plugin::ui::tree_json(view().remove(0));
        assert_eq!(wide["type"], "Split");
        assert_eq!(wide["props"]["gap"], "l", "the panes' cards keep off the divider");
        assert!(slot(&wide, "start").contains("Session preview") && !slot(&wide, "end").contains("Session preview"));
        NARROW.with(|narrow| narrow.set(true));
        let narrow = wings_plugin::ui::tree_json(view().remove(0));
        assert!(!slot(&narrow, "start").contains("Session preview") && slot(&narrow, "end").contains("Session preview"));
        NARROW.with(|narrow| narrow.set(false));
    }

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
    fn each_fold_tick_says_what_it_is_and_previews_the_element_it_opens_or_shuts() {
        use crate::settings_preview;
        let json = |node| wings_plugin::ui::tree_json(node).to_string();
        // A kind's preview holds its fold's body only when ticked.
        let samples = [
            (Kind::Steps, "cargo test parser"),
            (Kind::Command, "test result: FAILED"),
            (Kind::Read, "fn parse"),
            (Kind::Edit, "trim_end()"),
            (Kind::Search, "src/lexer.rs:9"),
            (Kind::Web, "The parser guide"),
            (Kind::Subagent, "Two checks skip"),
            (Kind::Other, "Reviewed 3 files"),
            (Kind::Thought, "expects the trailing newline"),
            (Kind::Summary, "two cases skipped"),
            (Kind::Note, "formatted 2 files"),
        ];
        assert_eq!(samples.len(), KINDS.len());
        for (kind, body) in samples {
            let (open, shut) = (json(settings_preview::kind(kind, true)), json(settings_preview::kind(kind, false)));
            assert!(open.contains(body) && !shut.contains(body), "{kind:?}: {open} / {shut}");
        }
        assert!(json(settings_preview::kind(Kind::Summary, true)).contains("Audit the discount code\\\" finished"));
        // A report shows whole when open and as its first line when shut.
        let (open, shut) = (json(settings_preview::report(true)), json(settings_preview::report(false)));
        assert!(open.contains("Both are fixed") && !shut.contains("Both are fixed") && shut.contains("Two checks skip the expiry date."));
        // The page has a labelled row and a description for each, the report's next to the summaries'.
        let page = json(stack().children(view()));
        for (kind, _, label) in KINDS {
            let (label, detail) = kind_text(kind, label);
            assert!(page.contains(label) && detail.ends_with('.') && page.contains(&detail.replace('"', "\\\"")), "{label}");
        }
        let (summary, report) = (page.find("Summaries of finished agents and commands").unwrap(), page.find("\"Subagent reports\"").unwrap());
        assert!(summary < report && report < page.find("Notes from Claude Code").unwrap());
        assert!(page.contains("What starts open") && page.contains("Every view mode"));
    }

    #[test]
    fn the_report_tick_is_open_when_ticked_and_keeps_the_fold_setting_as_it_was() {
        // Folding is off by default, so the tick starts ticked.
        assert!(!prefs::fold_reports().get());
        let page = wings_plugin::ui::tree_json(stack().children(view()));
        fn tick(node: &serde_json::Value) -> Option<bool> {
            if node["type"] == "Checkbox" && node.to_string().contains("Subagent reports") {
                return node["props"]["checked"].as_bool();
            }
            node["children"].as_array()?.iter().find_map(tick)
        }
        assert_eq!(tick(&page), Some(true));
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
    fn the_background_is_a_choice_of_the_plain_ground_pixel_drift_or_a_gradient_and_starts_on_the_ordered_dither() {
        fn select_named<'a>(node: &'a serde_json::Value, label: &str) -> Option<&'a serde_json::Value> {
            if node["type"] == "Select" && node["props"]["label"] == label {
                return Some(node);
            }
            node["children"].as_array()?.iter().find_map(|child| select_named(child, label))
        }
        let page = wings_plugin::ui::tree_json(view().remove(0));
        let select = select_named(&page, "Background").expect("the background's choice");
        assert_eq!(select["props"]["value"], "pixels:bayer");
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
