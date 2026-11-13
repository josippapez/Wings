//! Where a project's issues live. A worktree Wings made for an agent is a project of its own, but its issues belong to the
//! project it is a worktree of: one store, one set of keys, one sidebar section.

use serde_json::Value;
use wings_plugin::host;

#[derive(Clone, Debug, PartialEq)]
pub struct Home {
    pub id: String,
    pub name: String,
}

fn field(project: &Value, name: &str) -> String {
    project[name].as_str().unwrap_or_default().to_owned()
}

/// The project whose store holds `project`'s issues: its main project when it is a worktree Wings made, else itself. A
/// parent Wings no longer lists leaves the worktree its own store: nothing else could name it.
pub fn home_in(project: &Value, projects: &[Value]) -> Home {
    let own = Home { id: field(project, "id"), name: field(project, "name") };
    match project["parentId"].as_str() {
        Some(parent) => projects.iter().find(|p| p["id"] == parent).map_or(own, |p| Home { id: field(p, "id"), name: field(p, "name") }),
        None => own,
    }
}

/// `home_in` against the projects Wings lists now. The one place every tool, hook and page asks.
pub async fn home_of(project: &Value) -> Home {
    if project["parentId"].is_string() {
        let projects = host::projects().await.unwrap_or_default();
        return home_in(project, projects.as_array().map_or(&[], Vec::as_slice));
    }
    Home { id: field(project, "id"), name: field(project, "name") }
}

/// The project `reference` names, by folder path, id or name (any case), for reading another project's issues.
pub fn pick_project<'a>(projects: &'a [Value], reference: &str) -> Result<&'a Value, String> {
    let want = reference.trim().trim_end_matches('/');
    if let Some(found) = projects.iter().find(|p| field(p, "path").trim_end_matches('/') == want || field(p, "id") == want) {
        return Ok(found);
    }
    let named: Vec<&Value> = projects.iter().filter(|p| p["parentId"].is_null() && field(p, "name").to_lowercase() == want.to_lowercase()).collect();
    match named.as_slice() {
        [one] => Ok(one),
        [] => {
            let names: Vec<String> = projects.iter().filter(|p| p["parentId"].is_null()).map(|p| field(p, "name")).collect();
            Err(format!("No project \"{want}\" in Wings. The projects are: {}.", if names.is_empty() { "none yet".to_owned() } else { names.join(", ") }))
        }
        many => Err(format!("{} projects are named \"{want}\". Pass the folder path of one: {}.", many.len(), many.iter().map(|p| field(p, "path")).collect::<Vec<_>>().join(", "))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_worktree_wings_made_keeps_its_issues_in_its_main_project_a_plain_project_is_its_own_home_and_a_worktree_whose_main_is_gone_keeps_its_own() {
        let web = json!({ "id": "p1", "name": "acme-web" });
        let tree = json!({ "id": "p2", "name": "acme-web-fix", "parentId": "p1" });
        assert_eq!(home_in(&tree, &[web.clone(), tree.clone()]), Home { id: "p1".into(), name: "acme-web".into() });
        assert_eq!(home_in(&web, &[web.clone(), tree.clone()]), Home { id: "p1".into(), name: "acme-web".into() });
        assert_eq!(home_in(&tree, std::slice::from_ref(&tree)), Home { id: "p2".into(), name: "acme-web-fix".into() });
    }

    #[test]
    fn a_project_to_read_is_picked_by_path_id_or_name_never_a_worktree_by_name() {
        let projects = [
            json!({ "id": "a", "name": "acme-web", "path": "/code/acme-web" }),
            json!({ "id": "b", "name": "acme-api", "path": "/code/acme-api" }),
            json!({ "id": "w", "name": "acme-web", "path": "/code/acme-web-worktrees/fix", "parentId": "a" }),
        ];
        let id = |reference: &str| pick_project(&projects, reference).unwrap()["id"].as_str().unwrap().to_owned();
        assert_eq!(id("/code/acme-api/"), "b");
        assert_eq!(id("ACME-WEB"), "a");
        assert_eq!(id("w"), "w");
        assert_eq!(pick_project(&projects, "nope").unwrap_err(), "No project \"nope\" in Wings. The projects are: acme-web, acme-api.");
    }
}
