//! The tools Claude, and `wings share <tool> key=value`, call. Each takes the input schema the manifest's `mcpTools` has
//! for it.

use crate::{
    approvals::{self, Answer},
    main_frame::{changed, listening_ports, refresh, take_down, with},
    shares::{self, Share},
    tunnel::{self, Change, Fail},
};
use serde_json::{Value, json};
use wings_plugin::tools;

const MANIFEST: &str = include_str!("../../wings-plugin.json");

/// The tool's input schema from the manifest.
fn schema(name: &str) -> Value {
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap_or_default();
    manifest["contributes"]["mcpTools"].as_array().and_then(|list| list.iter().find(|tool| tool["name"] == name)).map_or(Value::Null, |tool| tool["inputSchema"].clone())
}

/// What a failed `opentunnel` call tells Claude. A missing one also opens the modal, which offers Install, and puts an
/// Install badge on the terminal Claude runs in: a Claude chat has none.
fn failed(fail: Fail, context: &Value) -> String {
    if fail == Fail::Missing {
        if let Some(pane) = context["paneId"].as_str() {
            with(|state| state.install_pane = Some(pane.to_owned()));
            drop(refresh());
        }
        crate::main_frame::set_missing(true);
        approvals::opened_by_user();
        wings_plugin::task::spawn(async {
            drop(wings_plugin::host::open_view_with(approvals::VIEW, "modal", &Value::Null).await);
        });
    }
    fail.message()
}

fn denied(answer: Answer, port: u16) -> Result<(), String> {
    let nothing = "Nothing was shared.";
    match answer {
        Answer::Allow => Ok(()),
        Answer::Deny => Err(format!("The user denied sharing localhost:{port} in Wings. {nothing}")),
        Answer::Timeout => Err(format!("The user didn't answer in Wings within 100 seconds. {nothing} Ask them before you try again.")),
        Answer::Unshown => Err(format!("Wings couldn't show the dialog that asks the user to allow it. {nothing}")),
    }
}

/// Shares `localhost:<port>` at a public link once the user allows it, and answers with the link.
pub async fn share_port(input: &Value, context: &Value, ask: impl AsyncFn(u16, Option<String>) -> Answer) -> Result<Value, String> {
    let port = tunnel::port_of(&input["port"])?;
    let name = match input["name"].as_str() {
        Some(name) => {
            tunnel::check_name(name)?;
            Some(name)
        }
        None => None,
    };
    // Before the user is asked: a missing opentunnel, or a name that is taken, can't be shared.
    let routes = tunnel::parse_list(&tunnel::run(tunnel::list_args(), tunnel::QUICK_MS).await.map_err(|fail| failed(fail, context))?);
    crate::main_frame::set_missing(false);
    if let Some(taken) = name.and_then(|name| routes.iter().find(|route| route.name == name && route.port != Some(port))) {
        return Err(format!("The name {} already points at {}. Pick another name, or end that one first with stop_share.", taken.name, taken.target));
    }
    // One answer is for the port for as long as a link Wings made for it stays: a share that failed, or a route that was
    // there before, keeps nothing, so the next share asks again.
    if !shares::all().iter().any(|share| share.port == port) {
        denied(ask(port, context["session"]["name"].as_str().map(str::to_owned)).await, port)?;
    }
    let out = tunnel::run(tunnel::add_args(port, name), tunnel::ADD_MS).await.map_err(|fail| match fail {
        Fail::Failed(message) if message.contains("timed out") => format!("{message}. The first link makes a tunnel, which can take a few minutes: call share_port again, it carries on."),
        fail => failed(fail, context),
    })?;
    let added = tunnel::parse_add(&out)?;
    let ours = shares::all().iter().any(|share| share.name == added.name);
    // A route that was there already, which the user made, isn't Wings' to take down.
    let owned = ours || added.change == Change::Added;
    let seen = listening_ports().await.is_some_and(|open| open.iter().any(|(_, open)| *open == port));
    if owned {
        shares::put(Share { name: added.name.clone(), port, url: added.url.clone(), seen: seen || shares::all().iter().any(|share| share.name == added.name && share.seen) });
        changed();
        drop(refresh());
    }
    let mut note = tunnel::note(&added.host);
    if !owned {
        note.push_str(" This route existed before, so Wings won't take it down by itself.");
    } else if !seen {
        note.push_str(&format!(" Wings sees no program listening on port {port} in a terminal, so the link stays until stop_share."));
    }
    Ok(json!({ "url": added.url, "host": added.host, "port": port, "name": added.name, "note": note }))
}

/// Every route OpenTunnel has, with the port each points at and whether Wings made it.
pub async fn list_shares(context: &Value) -> Result<Value, String> {
    let routes = tunnel::parse_list(&tunnel::run(tunnel::list_args(), tunnel::QUICK_MS).await.map_err(|fail| failed(fail, context))?);
    if routes.is_empty() {
        return Ok(Value::String("Nothing is shared.".into()));
    }
    let ours = shares::all();
    Ok(Value::Array(
        routes
            .iter()
            .map(|route| json!({ "name": route.name, "url": route.url(), "target": route.target, "port": route.port, "madeHere": ours.iter().any(|share| share.name == route.name) }))
            .collect(),
    ))
}

/// Ends the link called `name`, or every link to `port`.
pub async fn stop_share(input: &Value, context: &Value) -> Result<Value, String> {
    let names: Vec<String> = match (input["name"].as_str(), input.get("port").filter(|port| !port.is_null())) {
        (Some(name), None) => {
            tunnel::check_name(name)?;
            vec![name.to_owned()]
        }
        (None, Some(port)) => {
            let port = tunnel::port_of(port)?;
            let routes = tunnel::parse_list(&tunnel::run(tunnel::list_args(), tunnel::QUICK_MS).await.map_err(|fail| failed(fail, context))?);
            let found: Vec<String> = routes.iter().filter(|route| route.port == Some(port)).map(|route| route.name.clone()).collect();
            if found.is_empty() {
                return Err(format!("Nothing is shared on port {port}. list_shares shows what is."));
            }
            found
        }
        _ => return Err("Give name or port, one of them.".into()),
    };
    let ended = take_down(&names).await.map_err(|fail| failed(fail, context))?;
    Ok(Value::String(format!("Stopped sharing {}.", ended.join(", "))))
}

pub fn register_all() {
    tools::register("share_port", schema("share_port"), |input, context| async move { share_port(&input, &context, |port, asker| approvals::ask(asker, port)).await });
    tools::register("list_shares", schema("list_shares"), |_, context| async move { list_shares(&context).await });
    tools::register("stop_share", schema("stop_share"), |input, context| async move { stop_share(&input, &context).await });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::main_frame::State;
    use std::{cell::RefCell, rc::Rc};
    use wings_plugin::{host, task::block_on};

    /// What opentunnel is made to print, and what its callers asked it.
    struct Fake {
        routes: Rc<RefCell<Vec<(String, String)>>>,
        calls: Rc<RefCell<Vec<Vec<String>>>>,
    }

    /// A fake `opentunnel`, over the routes `routes` (name, target), that keeps the routes `add` and `remove` change and
    /// every call. `ports` are what Wings sees listening.
    fn opentunnel(routes: &[(&str, &str)], ports: &[u64]) -> Fake {
        with(|state| *state = State::default());
        shares::save(vec![]);
        let routes: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(routes.iter().map(|(name, target)| ((*name).to_owned(), (*target).to_owned())).collect()));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let (state, kept) = (routes.clone(), calls.clone());
        let listening: Vec<Value> = ports.iter().map(|port| json!({ "paneId": "1", "pid": 7, "name": "node", "port": port, "startedAt": 0 })).collect();
        host::testing::answer(move |method, params| match method {
            "ports" => Ok(Value::Array(listening.clone())),
            "exec" => {
                assert_eq!(params["program"], "opentunnel");
                let args: Vec<String> = params["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_owned()).collect();
                kept.borrow_mut().push(args.clone());
                let out = |stdout: String| Ok(json!({ "code": 0, "stdout": stdout, "stderr": "" }));
                let mut routes = state.borrow_mut();
                match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
                    ["route", "list"] if routes.is_empty() => out("No routes configured.\n".into()),
                    ["route", "list"] => out(routes.iter().map(|(name, target)| format!("{name}.abc123.opentunnel.xyz  →  {target}\n")).collect()),
                    ["route", "add", port, rest @ ..] => {
                        let target = format!("127.0.0.1:{port}");
                        let name = match rest {
                            ["--name", name] => (*name).to_owned(),
                            _ => routes.iter().find(|(_, t)| *t == target).map_or_else(|| format!("{:016x}", port.parse::<u32>().unwrap()), |(n, _)| n.clone()),
                        };
                        let said = match routes.iter_mut().find(|(n, _)| *n == name) {
                            None => {
                                routes.push((name.clone(), target.clone()));
                                format!("Added route {name} → {target}")
                            }
                            Some((_, t)) if *t == target => format!("Route {name} → {target} already exists"),
                            Some((_, t)) => {
                                let was = std::mem::replace(t, target.clone());
                                format!("Updated route {name} → {target} (was {was})")
                            }
                        };
                        out(format!("{said}\nhttps://{name}.abc123.opentunnel.xyz\n"))
                    }
                    ["route", "remove", name] => match routes.iter().position(|(n, _)| n == name) {
                        Some(at) => {
                            routes.remove(at);
                            out(format!("Removed route {name}.abc123.opentunnel.xyz\n"))
                        }
                        None => Ok(json!({ "code": 1, "stdout": "", "stderr": format!("Error: no route named '{name}' in profile default\n") })),
                    },
                    other => panic!("unexpected {other:?}"),
                }
            }
            _ => Ok(Value::Null),
        });
        Fake { routes, calls }
    }

    fn ask_with(answer: Answer) -> impl AsyncFn(u16, Option<String>) -> Answer {
        async move |_, _| answer
    }

    #[test]
    fn sharing_a_port_adds_a_route_for_the_bare_port_and_answers_with_the_link_and_a_note() {
        let fake = opentunnel(&[], &[3000]);
        let said = block_on(async { share_port(&json!({ "port": 3000 }), &json!({}), ask_with(Answer::Allow)).await }).unwrap();
        assert_eq!(said["url"], "https://0000000000000bb8.abc123.opentunnel.xyz");
        assert_eq!((said["host"].clone(), said["port"].clone(), said["name"].clone()), (json!("0000000000000bb8.abc123.opentunnel.xyz"), json!(3000), json!("0000000000000bb8")));
        let note = said["note"].as_str().unwrap();
        assert!(note.contains("Anyone with this link") && note.contains("Vite") && note.contains("0000000000000bb8.abc123.opentunnel.xyz"), "{note}");
        // The port listens in a Wings terminal, so nothing more is said.
        assert!(!note.contains("sees no program"), "{note}");
        assert_eq!(fake.calls.borrow().last().unwrap(), &["route", "add", "3000"]);
        assert_eq!(shares::all(), [Share { name: "0000000000000bb8".into(), port: 3000, url: said["url"].as_str().unwrap().into(), seen: true }]);
    }

    #[test]
    fn a_name_goes_to_the_route_and_a_bad_one_never_reaches_opentunnel() {
        let fake = opentunnel(&[], &[]);
        let said = block_on(async { share_port(&json!({ "port": 3000, "name": "demo" }), &json!({}), ask_with(Answer::Allow)).await }).unwrap();
        assert_eq!(said["url"], "https://demo.abc123.opentunnel.xyz");
        assert_eq!(fake.calls.borrow().last().unwrap(), &["route", "add", "3000", "--name", "demo"]);
        let calls = fake.calls.borrow().len();
        for bad in [json!({ "port": 0 }), json!({ "port": 70000 }), json!({ "port": "127.0.0.1:3000" }), json!({ "port": 3000, "name": "My App" }), json!({ "port": 3000, "name": "a;b" })] {
            let input = bad.clone();
            assert!(block_on(async move { share_port(&input, &json!({}), ask_with(Answer::Allow)).await }).is_err(), "{bad}");
        }
        assert_eq!(fake.calls.borrow().len(), calls, "a bad input ran opentunnel");
    }

    #[test]
    fn a_name_that_points_at_another_port_is_refused_before_the_user_is_asked() {
        let fake = opentunnel(&[("demo", "127.0.0.1:4000")], &[]);
        let asked = Rc::new(std::cell::Cell::new(false));
        let kept = asked.clone();
        let error = block_on(async move {
            share_port(&json!({ "port": 3000, "name": "demo" }), &json!({}), async |_, _| {
                kept.set(true);
                Answer::Allow
            })
            .await
        })
        .unwrap_err();
        assert!(error.contains("demo") && error.contains("127.0.0.1:4000"), "{error}");
        assert!(!asked.get());
        assert!(fake.calls.borrow().iter().all(|call| call[1] != "add"));
    }

    #[test]
    fn nothing_is_shared_when_the_user_denies_or_does_not_answer() {
        for (answer, said) in [(Answer::Deny, "denied"), (Answer::Timeout, "didn't answer"), (Answer::Unshown, "couldn't show")] {
            let fake = opentunnel(&[], &[3000]);
            let error = block_on(async move { share_port(&json!({ "port": 3000 }), &json!({}), ask_with(answer)).await }).unwrap_err();
            assert!(error.contains(said) && error.contains("Nothing was shared"), "{error}");
            assert!(fake.calls.borrow().iter().all(|call| call[1] != "add"));
            assert!(shares::all().is_empty());
        }
    }

    #[test]
    fn the_user_is_asked_once_for_a_port_while_it_stays_shared() {
        opentunnel(&[], &[3000]);
        let asks = Rc::new(std::cell::Cell::new(0));
        let counted = asks.clone();
        let share = |port: u64| {
            let counted = counted.clone();
            block_on(async move {
                share_port(&json!({ "port": port }), &json!({}), async |_, _| {
                    counted.set(counted.get() + 1);
                    Answer::Allow
                })
                .await
            })
            .unwrap()
        };
        let first = share(3000);
        let second = share(3000);
        assert_eq!((asks.get(), first["url"].clone()), (1, second["url"].clone()));
        share(4000);
        assert_eq!(asks.get(), 2);
        // Once it is stopped, sharing it again is asked about again.
        block_on(async { stop_share(&json!({ "port": 3000 }), &json!({})).await }).unwrap();
        share(3000);
        assert_eq!(asks.get(), 3);
    }

    #[test]
    fn a_port_wings_does_not_see_is_shared_with_a_note_that_it_stays_until_stopped() {
        opentunnel(&[], &[]);
        let said = block_on(async { share_port(&json!({ "port": 8080 }), &json!({}), ask_with(Answer::Allow)).await }).unwrap();
        assert!(said["note"].as_str().unwrap().contains("stays until stop_share"));
        assert!(!shares::all()[0].seen);
    }

    #[test]
    fn a_route_the_user_made_is_shared_but_not_kept_to_take_down() {
        opentunnel(&[("mine", "127.0.0.1:3000")], &[3000]);
        let said = block_on(async { share_port(&json!({ "port": 3000 }), &json!({}), ask_with(Answer::Allow)).await }).unwrap();
        assert_eq!(said["name"], "mine");
        assert!(said["note"].as_str().unwrap().contains("existed before"));
        assert!(shares::all().is_empty());
    }

    #[test]
    fn a_missing_opentunnel_is_an_error_that_says_how_to_install_it_and_badges_the_asking_pane() {
        with(|state| *state = State::default());
        host::testing::answer(|method, _| match method {
            "exec" => Err("opentunnel is not installed or not on PATH".into()),
            _ => Ok(Value::Null),
        });
        let asked = Rc::new(std::cell::Cell::new(false));
        let kept = asked.clone();
        let error = block_on(async move {
            share_port(&json!({ "port": 3000 }), &json!({ "paneId": "7" }), async |_, _| {
                kept.set(true);
                Answer::Allow
            })
            .await
        })
        .unwrap_err();
        assert!(error.contains("isn't installed") && error.contains("brew install anomalyco/tap/opentunnel"), "{error}");
        assert!(!asked.get(), "the user was asked about something that can't work");
        assert_eq!(with(|state| state.install_pane.clone()), Some("7".into()));
        // The modal opens to offer Install, as a chat has no terminal for the badge.
        let shown = Rc::new(RefCell::new(Vec::new()));
        let kept = shown.clone();
        host::testing::answer(move |method, params| {
            kept.borrow_mut().push((method.to_owned(), params.clone()));
            match method {
                "exec" => Err("opentunnel is not installed or not on PATH".into()),
                _ => Ok(Value::Null),
            }
        });
        crate::main_frame::set_missing(false);
        let _ = block_on(async { share_port(&json!({ "port": 3000 }), &json!({}), ask_with(Answer::Allow)).await });
        assert!(shown.borrow().iter().any(|(method, params)| method == "openView" && params["id"] == "share"), "{:?}", shown.borrow());
        assert_eq!(crate::main_frame::state_message()["missing"], true, "the modal offers Install");
        for tool in [block_on(async { list_shares(&json!({})).await }), block_on(async { stop_share(&json!({ "name": "x" }), &json!({})).await })] {
            assert!(tool.unwrap_err().contains("brew install"));
        }
    }

    #[test]
    fn list_shares_gives_each_route_with_its_port_and_whether_wings_made_it() {
        opentunnel(&[("mine", "127.0.0.1:3000"), ("db", "10.0.0.5:5432")], &[]);
        shares::put(Share { name: "mine".into(), port: 3000, url: String::new(), seen: true });
        let list = block_on(async { list_shares(&json!({})).await }).unwrap();
        assert_eq!(list[0], json!({ "name": "mine", "url": "https://mine.abc123.opentunnel.xyz", "target": "127.0.0.1:3000", "port": 3000, "madeHere": true }));
        assert_eq!((list[1]["port"].clone(), list[1]["madeHere"].clone()), (Value::Null, json!(false)));
        opentunnel(&[], &[]);
        assert_eq!(block_on(async { list_shares(&json!({})).await }).unwrap(), "Nothing is shared.");
    }

    #[test]
    fn stopping_by_name_or_by_port_removes_the_route_and_forgets_the_link() {
        let fake = opentunnel(&[], &[3000, 4000]);
        block_on(async { share_port(&json!({ "port": 3000, "name": "a" }), &json!({}), ask_with(Answer::Allow)).await }).unwrap();
        block_on(async { share_port(&json!({ "port": 4000, "name": "b" }), &json!({}), ask_with(Answer::Allow)).await }).unwrap();
        let said = block_on(async { stop_share(&json!({ "name": "a" }), &json!({})).await }).unwrap();
        assert_eq!(said, "Stopped sharing a.");
        assert_eq!(fake.calls.borrow().last().unwrap(), &["route", "remove", "a"]);
        assert_eq!(shares::all().iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["b"]);
        block_on(async { stop_share(&json!({ "port": 4000 }), &json!({})).await }).unwrap();
        assert!(shares::all().is_empty() && fake.routes.borrow().is_empty());
        let error = block_on(async { stop_share(&json!({ "port": 4000 }), &json!({})).await }).unwrap_err();
        assert!(error.contains("Nothing is shared on port 4000"), "{error}");
        assert!(block_on(async { stop_share(&json!({}), &json!({})).await }).is_err());
        assert!(block_on(async { stop_share(&json!({ "name": "a", "port": 3000 }), &json!({})).await }).is_err());
        assert!(block_on(async { stop_share(&json!({ "name": "A b" }), &json!({})).await }).is_err());
    }

    #[test]
    fn a_port_that_closed_comes_down_after_two_looks_and_only_its_own_link() {
        let fake = opentunnel(&[("mine", "127.0.0.1:9000")], &[3000, 4000]);
        block_on(async { share_port(&json!({ "port": 3000, "name": "a" }), &json!({}), ask_with(Answer::Allow)).await }).unwrap();
        block_on(async { share_port(&json!({ "port": 4000, "name": "b" }), &json!({}), ask_with(Answer::Allow)).await }).unwrap();
        // 3000 closes: Wings now sees 4000 only.
        let closed = |open: &'static [u64]| {
            let calls = fake.calls.clone();
            let routes = fake.routes.clone();
            host::testing::answer(move |method, params| match method {
                "ports" => Ok(Value::Array(open.iter().map(|port| json!({ "paneId": "1", "port": port })).collect())),
                "exec" => {
                    calls.borrow_mut().push(params["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_owned()).collect());
                    routes.borrow_mut().retain(|(name, _)| params["args"][2] != name.as_str());
                    Ok(json!({ "code": 0, "stdout": "Removed route x\n", "stderr": "" }))
                }
                _ => Ok(Value::Null),
            });
            block_on(crate::main_frame::look());
        };
        closed(&[4000]);
        assert_eq!(shares::all().len(), 2, "one look is not enough");
        closed(&[4000]);
        assert_eq!(shares::all().iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["b"]);
        assert!(fake.routes.borrow().iter().any(|(name, _)| name == "mine"), "a route the user made was removed");
        assert!(fake.routes.borrow().iter().all(|(name, _)| name != "a"));
        assert_eq!(fake.calls.borrow().iter().filter(|call| call[1] == "remove").count(), 1);
    }

    /// An Allow counts only while a link Wings made is up: after a route the user made before, or a failed add, the next
    /// share of the port asks again.
    #[test]
    fn a_share_that_kept_no_link_of_wings_asks_again_next_time() {
        let fake = opentunnel(&[("mine", "127.0.0.1:3000")], &[3000]);
        let asks = Rc::new(std::cell::Cell::new(0));
        let share = |asks: Rc<std::cell::Cell<u32>>| {
            block_on(async move {
                share_port(&json!({ "port": 3000 }), &json!({}), async |_, _| {
                    asks.set(asks.get() + 1);
                    Answer::Allow
                })
                .await
            })
        };
        share(asks.clone()).unwrap();
        assert!(shares::all().is_empty(), "the route was the user's");
        share(asks.clone()).unwrap();
        assert_eq!(asks.get(), 2);
        // The add fails: nothing kept, asked again.
        fake.routes.borrow_mut().clear();
        host::testing::answer(|method, params| match (method, params["args"][1].as_str()) {
            ("exec", Some("list")) => Ok(json!({ "code": 0, "stdout": "No routes configured.\n", "stderr": "" })),
            ("exec", _) => Ok(json!({ "code": 1, "stdout": "", "stderr": "Error: the service could not start\n" })),
            _ => Ok(json!([])),
        });
        assert!(share(asks.clone()).is_err());
        assert!(share(asks.clone()).is_err());
        assert_eq!(asks.get(), 4);
    }

    #[test]
    fn a_link_whose_route_is_already_gone_is_forgotten() {
        let fake = opentunnel(&[], &[3000]);
        block_on(async { share_port(&json!({ "port": 3000, "name": "a" }), &json!({}), ask_with(Answer::Allow)).await }).unwrap();
        fake.routes.borrow_mut().clear();
        block_on(async { stop_share(&json!({ "name": "a" }), &json!({})).await }).unwrap();
        assert!(shares::all().is_empty());
    }
}
