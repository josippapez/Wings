//! The "Public links" modal. Main opens it when Claude asks to share a port, with Allow and Deny, and the badge of a shared
//! pane opens it to copy the link. It lists what is shared, with Copy link, Open and Stop.

use crate::{
    ASK, DONE, STATE, WANT,
    shares::Share,
};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use wings_plugin::{
    host, messages,
    task::spawn,
    ui::{Node, button, card, empty, loading, muted, notice, row, slot, stack, text},
};

/// Allow waits this long after an ask shows, so a click meant for something else can't land on it.
const ARM_MS: u32 = 1000;

struct Approval {
    id: String,
    port: u64,
    asker: Option<String>,
}

#[derive(Default)]
struct Page {
    /// What main said, once it has: the asks that wait, oldest first, and the links.
    approvals: Option<Vec<Approval>>,
    shares: Vec<Share>,
    /// Whether OpenTunnel was missing when Claude last tried to share, so Install shows.
    missing: bool,
    /// The ask that is first, which Allow waits on.
    shown: Option<String>,
    armed: bool,
    /// The link whose Copy link was pressed last.
    copied: Option<String>,
    /// Whether main is busy with a change this page asked for.
    busy: bool,
    error: Option<String>,
}

thread_local! {
    static PAGE: RefCell<Page> = RefCell::new(Page::default());
    static NEXT: Cell<u32> = const { Cell::new(0) };
}

fn with<T>(change: impl FnOnce(&mut Page) -> T) -> T {
    PAGE.with(|page| change(&mut page.borrow_mut()))
}

/// Asks main for `op` with `args`: the page is busy until it answers, and an error shows.
fn ask(op: &str, mut args: Value) {
    let id = NEXT.with(|next| {
        next.set(next.get() + 1);
        format!("p{}", next.get())
    });
    args["id"] = id.into();
    args["op"] = op.into();
    with(|page| (page.busy, page.error) = (true, None));
    messages::tell(ASK, &args);
}

fn small(label: &str, variant: &str, disabled: bool, on_click: impl Fn() + 'static) -> Node {
    let button = button(label, variant, false, on_click).prop("size", "sm");
    if disabled { button.prop("disabled", true) } else { button }
}

fn approval_card(first: &Approval, more: usize, busy: bool, armed: bool) -> Node {
    let who = first.asker.as_ref().map_or("Claude".to_owned(), |asker| format!("\"{asker}\""));
    let (deny, allow) = (first.id.clone(), first.id.clone());
    stack()
        .prop("gap", "m")
        .child(text(format!("Share localhost:{} publicly?", first.port)).prop("weight", "strong"))
        .child(muted(format!("{who} asks for a link to the app that runs on port {}.{}", first.port, if more > 0 { format!(" {more} more waiting.") } else { String::new() })))
        .child(muted("Anyone who has the link can open the app, even where its own page says it is only for this computer. The link works until you stop it or the app stops.").prop("size", "small"))
        .child(wings_plugin::ui::actions(
            Some(small("Deny", "ghost", busy, move || ask("answer", json!({ "approval": deny, "allow": false })))),
            [small("Allow", "default", busy || !armed, move || ask("answer", json!({ "approval": allow, "allow": true })))],
        ))
}

/// OpenTunnel isn't installed: what it is, and Install, which runs the Homebrew command in a terminal tab.
fn install_card(busy: bool) -> Node {
    card().child(
        stack()
            .prop("gap", "m")
            .child(text("OpenTunnel isn't installed").prop("weight", "strong"))
            .child(muted("Sharing a link needs OpenTunnel, a free command-line tool. Install runs this in a new terminal tab, then ask Claude again:"))
            .child(text("brew install anomalyco/tap/opentunnel").prop("mono", true).prop("size", "small"))
            .child(wings_plugin::ui::actions(None, [small("Install", "default", busy, || ask("install", json!({})))])),
    )
}

fn share_card(share: &Share, copied: bool, busy: bool) -> Node {
    let (url, open, name) = (share.url.clone(), share.url.clone(), share.name.clone());
    let copy_label = if copied { "Copied" } else { "Copy link" };
    card().key(share.name.clone()).child(
        stack()
            .prop("gap", "s")
            .child(text(share.url.as_str()).prop("weight", "strong").prop("size", "small"))
            .child(muted(format!("localhost:{}", share.port)).prop("size", "small"))
            .child(
                row()
                    .prop("gap", "s")
                    .prop("wrap", true)
                    .child(small(copy_label, "secondary", false, move || {
                        drop(host::copy_text(&url));
                        let name = name.clone();
                        with(|page| page.copied = Some(name));
                    }))
                    .child(small("Open", "secondary", false, move || drop(host::call("openUrl", json!({ "url": open })))))
                    .child(small("Stop sharing", "ghost", busy, {
                        let name = share.name.clone();
                        move || ask("stop", json!({ "name": name }))
                    })),
            ),
    )
}

fn view() -> Vec<Node> {
    PAGE.with(|state| {
        let p = &*state.borrow();
        let Some(approvals) = &p.approvals else {
            return vec![loading()];
        };
        if approvals.is_empty() && p.shares.is_empty() && !p.missing {
            return vec![empty("Nothing is shared", "Ask Claude to share a running app. Each link shows here, with a button to copy it.")];
        }
        let body = stack()
            .prop("gap", "m")
            .maybe(p.missing.then(|| install_card(p.busy)))
            .maybe(approvals.first().map(|first| card().child(approval_card(first, approvals.len() - 1, p.busy, p.armed))))
            .maybe(p.error.clone().map(|error| notice("danger", error)))
            .maybe((!p.shares.is_empty()).then(|| {
                stack()
                    .prop("gap", "s")
                    .child(text("Shared now").prop("weight", "strong"))
                    .children(p.shares.iter().map(|share| share_card(share, p.copied.as_deref() == Some(share.name.as_str()), p.busy)))
            }));
        vec![Node::new("Page").child(slot("body", body))]
    })
}

/// Takes what main told. Allow for the first ask waits a second after it shows.
fn hear(state: &Value) {
    let approvals: Vec<Approval> = state["approvals"]
        .as_array()
        .map(|list| list.iter().filter_map(|a| Some(Approval { id: a["id"].as_str()?.to_owned(), port: a["port"].as_u64()?, asker: a["asker"].as_str().map(str::to_owned) })).collect())
        .unwrap_or_default();
    let first = approvals.first().map(|a| a.id.clone());
    let moved = with(|p| {
        let moved = p.shown != first;
        p.shares = state["shares"].as_array().map(|list| list.iter().filter_map(Share::from_json).collect()).unwrap_or_default();
        p.missing = state["missing"] == true;
        (p.approvals, p.shown) = (Some(approvals), first.clone());
        if moved {
            p.armed = false;
        }
        moved
    });
    if let (true, Some(first)) = (moved, first) {
        spawn(async move {
            host::sleep(ARM_MS).await;
            with(|p| p.armed = p.shown.as_ref() == Some(&first));
        });
    }
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run(view);
    messages::on(STATE, hear);
    messages::on(DONE, |done| {
        with(|p| {
            p.busy = false;
            p.error = done["error"].as_str().map(str::to_owned);
        })
    });
    messages::tell(WANT, &Value::Null);
}
