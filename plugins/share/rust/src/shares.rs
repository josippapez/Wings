//! The links this plugin made, which it keeps in the plugin's storage so a restart of Wings still knows them, and when
//! each is taken down. OpenTunnel's service keeps a route after Wings quits and after the program behind it stops, so
//! the plugin removes the ones it made once their port has closed. It never removes a route it didn't make.

use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
};
use wings_plugin::host::storage;

const KEY: &str = "shares";
/// How many looks in a row a port has to be gone before its link is taken down: a dev server that restarts for a changed
/// config closes its port for a moment, and a new link would have another address.
pub const GONE_LOOKS: u32 = 2;

#[derive(Clone, Debug, PartialEq)]
pub struct Share {
    /// What `route remove` takes.
    pub name: String,
    pub port: u16,
    pub url: String,
    /// Whether a program in a Wings terminal has listened on `port` since it was shared. Wings sees the ports of its own
    /// terminals only, so a link to a port it never saw stays until it is stopped.
    pub seen: bool,
}

impl Share {
    pub fn to_json(&self) -> Value {
        json!({ "name": self.name, "port": self.port, "url": self.url, "seen": self.seen })
    }

    pub fn from_json(value: &Value) -> Option<Self> {
        Some(Share { name: value["name"].as_str()?.to_owned(), port: u16::try_from(value["port"].as_u64()?).ok()?, url: value["url"].as_str()?.to_owned(), seen: value["seen"] == true })
    }
}

/// What a look at the open ports decides.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    /// Links whose port listens now and wasn't known to.
    pub seen: Vec<String>,
    /// Links whose port closed.
    pub remove: Vec<String>,
}

/// What to do with `shares` when `listening` are the ports open in Wings' terminals. `gone` counts, for each link, the
/// looks in a row its port was missing.
pub fn plan(shares: &[Share], listening: &BTreeSet<u16>, gone: &mut HashMap<String, u32>) -> Plan {
    gone.retain(|name, _| shares.iter().any(|share| &share.name == name));
    let mut plan = Plan::default();
    for share in shares {
        if listening.contains(&share.port) {
            gone.remove(&share.name);
            if !share.seen {
                plan.seen.push(share.name.clone());
            }
        } else if share.seen {
            let looks = gone.entry(share.name.clone()).or_default();
            *looks += 1;
            if *looks >= GONE_LOOKS {
                plan.remove.push(share.name.clone());
            }
        }
    }
    plan
}

thread_local! {
    static SHARES: RefCell<Option<Vec<Share>>> = const { RefCell::new(None) };
}

/// Every link, read from storage the first time.
pub fn all() -> Vec<Share> {
    SHARES.with(|shares| {
        shares
            .borrow_mut()
            .get_or_insert_with(|| storage::get(KEY).ok().and_then(|stored| stored.as_array().map(|list| list.iter().filter_map(Share::from_json).collect())).unwrap_or_default())
            .clone()
    })
}

/// Keeps `list` as the links.
pub fn save(list: Vec<Share>) {
    drop(storage::set(KEY, &Value::Array(list.iter().map(Share::to_json).collect())));
    SHARES.with(|shares| *shares.borrow_mut() = Some(list));
}

/// Adds the link, or replaces the one of its name.
pub fn put(share: Share) {
    let mut list = all();
    match list.iter_mut().find(|one| one.name == share.name) {
        Some(one) => *one = share,
        None => list.push(share),
    }
    save(list);
}

/// Forgets the links named in `names`.
pub fn forget(names: &[String]) {
    let mut list = all();
    list.retain(|share| !names.contains(&share.name));
    save(list);
}

/// Marks the links named in `names` as having had a program listening.
pub fn mark_seen(names: &[String]) {
    let mut list = all();
    list.iter_mut().filter(|share| names.contains(&share.name)).for_each(|share| share.seen = true);
    save(list);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn share(name: &str, port: u16, seen: bool) -> Share {
        Share { name: name.into(), port, url: format!("https://{name}.x.opentunnel.xyz"), seen }
    }

    fn listening(ports: &[u16]) -> BTreeSet<u16> {
        ports.iter().copied().collect()
    }

    #[test]
    fn a_link_whose_port_listens_is_kept_and_marked_seen_once() {
        let mut gone = HashMap::new();
        let shares = [share("a", 3000, false), share("b", 4000, true)];
        let plan = plan(&shares, &listening(&[3000, 4000]), &mut gone);
        assert_eq!(plan, Plan { seen: vec!["a".into()], remove: vec![] });
    }

    #[test]
    fn a_link_comes_down_when_its_port_has_been_gone_for_two_looks() {
        let mut gone = HashMap::new();
        let shares = [share("a", 3000, true)];
        assert_eq!(plan(&shares, &listening(&[]), &mut gone), Plan::default());
        assert_eq!(plan(&shares, &listening(&[]), &mut gone), Plan { seen: vec![], remove: vec!["a".into()] });
        // Still gone, still asked for, until it is taken down and forgotten.
        assert_eq!(plan(&shares, &listening(&[]), &mut gone).remove, ["a"]);
    }

    #[test]
    fn a_port_that_comes_back_for_a_restart_keeps_its_link() {
        let mut gone = HashMap::new();
        let shares = [share("a", 3000, true)];
        assert!(plan(&shares, &listening(&[]), &mut gone).remove.is_empty());
        assert!(plan(&shares, &listening(&[3000]), &mut gone).remove.is_empty());
        // The count starts again: one more look without the port isn't enough.
        assert!(plan(&shares, &listening(&[]), &mut gone).remove.is_empty());
    }

    #[test]
    fn a_link_to_a_port_wings_never_saw_is_never_taken_down() {
        let mut gone = HashMap::new();
        let shares = [share("outside", 8080, false)];
        for _ in 0..5 {
            assert_eq!(plan(&shares, &listening(&[3000]), &mut gone), Plan::default());
        }
    }

    #[test]
    fn only_the_link_whose_port_closed_comes_down() {
        let mut gone = HashMap::new();
        let shares = [share("a", 3000, true), share("b", 4000, true)];
        plan(&shares, &listening(&[4000]), &mut gone);
        assert_eq!(plan(&shares, &listening(&[4000]), &mut gone).remove, ["a"]);
    }

    #[test]
    fn the_counts_of_forgotten_links_are_dropped() {
        let mut gone = HashMap::new();
        plan(&[share("a", 3000, true)], &listening(&[]), &mut gone);
        plan(&[], &listening(&[]), &mut gone);
        assert!(gone.is_empty());
    }

    #[test]
    fn the_list_keeps_what_is_put_in_it_and_forgets_what_is_named() {
        save(vec![]);
        put(share("a", 3000, false));
        put(share("b", 4000, true));
        put(share("a", 3000, true));
        assert_eq!(all(), [share("a", 3000, true), share("b", 4000, true)]);
        forget(&["a".into()]);
        assert_eq!(all(), [share("b", 4000, true)]);
        assert_eq!(Share::from_json(&share("b", 4000, true).to_json()), Some(share("b", 4000, true)));
        save(vec![]);
    }
}
