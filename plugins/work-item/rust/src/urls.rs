//! Azure DevOps web links, and the time of a work item's update entries.

use serde_json::Value;

/// The URL prefix the manifest's `openUrl` allows: a visualstudio.com org also answers at dev.azure.com.
pub fn web_base(org: &str) -> String {
    wings_plugin::azure::api_org(org)
}

/// What `encodeURIComponent` leaves as it is: letters, digits and `-_.!~*'()`.
pub fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

pub fn work_item_url(org: &str, project: &str, id: u64) -> String {
    format!("{}/{}/_workitems/edit/{id}", web_base(org), encode(project))
}

pub fn pr_url(org: &str, project: &str, repo: &str, id: u64) -> String {
    format!("{}/{}/_git/{}/pullrequest/{id}", web_base(org), encode(project), encode(repo))
}

/// The time of one entry of `GET wit/workItems/{id}/updates`. The newest one's `revisedDate` is 9999-01-01, so the time
/// is its ChangedDate field.
pub fn update_time(update: &Value) -> String {
    let changed = &update["fields"]["System.ChangedDate"]["newValue"];
    changed.as_str().or_else(|| update["revisedDate"].as_str()).unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn links_name_the_project_and_repository_and_an_old_org_answers_at_dev_azure_com() {
        assert_eq!(work_item_url("https://acme.visualstudio.com", "My Proj", 5), "https://dev.azure.com/acme/My%20Proj/_workitems/edit/5");
        assert_eq!(pr_url("https://dev.azure.com/acme", "Web", "web", 482), "https://dev.azure.com/acme/Web/_git/web/pullrequest/482");
    }

    #[test]
    fn an_updates_time_is_its_changed_date_else_its_revised_date() {
        assert_eq!(update_time(&json!({ "revisedDate": "9999-01-01T00:00:00Z", "fields": { "System.ChangedDate": { "newValue": "2026-10-03T10:00:00Z" } } })), "2026-10-03T10:00:00Z");
        assert_eq!(update_time(&json!({ "revisedDate": "2026-10-02T00:00:00Z" })), "2026-10-02T00:00:00Z");
        assert_eq!(update_time(&json!({})), "");
    }
}
