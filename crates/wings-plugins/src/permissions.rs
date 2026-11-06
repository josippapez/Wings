//! The permissions that are a plain yes or no, what a plugin may call or hear from other plugins, and notifications.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use crate::manifest::Plugin;

/// Whether the plugin may see which local ports the programs in each pane listen on.
pub fn may_read_ports(plugin: &Plugin) -> bool {
    plugin.manifest.permissions.ports
}

/// Whether the plugin may read what an OpenCode session is doing.
/// Whether the plugin's `callTools` names this tool, or every tool of its plugin.
pub fn may_call_tool(plugin: &Plugin, target: &str, tool: &str) -> bool {
    plugin.manifest.permissions.call_tools.iter().any(|e| match e.split_once(' ') {
        Some((p, t)) => p == target && t == tool,
        None => e == target,
    })
}

/// Whether the plugin's `events` names this event of this plugin.
pub fn may_hear_event(plugin: &Plugin, from: &str, name: &str) -> bool {
    plugin.manifest.permissions.events.iter().any(|e| e.split_once(' ').is_some_and(|(p, n)| p == from && n == name))
}

pub fn may_create_worktrees(plugin: &Plugin) -> bool {
    plugin.manifest.permissions.worktrees
}

pub fn may_send_to_agents(plugin: &Plugin) -> bool {
    plugin.manifest.permissions.send_to_agents
}

pub fn may_read_agent_activity(plugin: &Plugin) -> bool {
    plugin.manifest.permissions.agent_activity
}

/// Whether the plugin may scan the disks for the names and sizes of their files and folders, and list them.
pub fn may_scan_disks(plugin: &Plugin) -> bool {
    plugin.manifest.permissions.disk_usage
}

/// Whether the plugin may read what Claude Code told `wings statusline`.
pub fn may_read_statusline(plugin: &Plugin) -> bool {
    plugin.manifest.permissions.statusline
}

/// Whether the plugin may open terminals and move focus between panes.
pub fn may_use_panes(plugin: &Plugin) -> bool {
    plugin.manifest.permissions.panes.is_some()
}

/// Notifications a plugin may show in any minute: enough for a burst of its own updates, too few to flood
/// Notification Center.
pub const NOTIFY_LIMIT: usize = 20;
const NOTIFY_WINDOW: Duration = Duration::from_secs(60);
const NOTIFY_TITLE_MAX: usize = 64;
const NOTIFY_BODY_MAX: usize = 256;

/// When each plugin showed its recent notifications, for the rate limit.
#[derive(Default)]
pub struct NotifyLog(HashMap<String, Vec<Instant>>);

impl NotifyLog {
    /// Counts a notification at `now` if the plugin is under the limit, else says how long until it can.
    pub fn allow(&mut self, plugin_id: &str, now: Instant) -> Result<(), Duration> {
        let sent = self.0.entry(plugin_id.to_string()).or_default();
        sent.retain(|t| now.duration_since(*t) < NOTIFY_WINDOW);
        if sent.len() >= NOTIFY_LIMIT {
            return Err(NOTIFY_WINDOW - now.duration_since(sent[0]));
        }
        sent.push(now);
        Ok(())
    }
}

pub fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max - 1).chain(['…']).collect()
}

/// The title and body to show, cut to length. The plugin's name leads the title, so a plugin can't pass
/// for Wings or Claude.
pub fn notification(plugin: &Plugin, title: &str, body: &str) -> Result<(String, String), String> {
    let id = &plugin.manifest.id;
    if !plugin.manifest.permissions.notify {
        return Err(format!("{id} may not show notifications"));
    }
    let title = title.trim();
    if title.is_empty() {
        return Err("A notification needs a title".into());
    }
    Ok((format!("{}: {}", plugin.manifest.name, clip(title, NOTIFY_TITLE_MAX)), clip(body.trim(), NOTIFY_BODY_MAX)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{loading::load, test_support::plugin};
    use serde_json::Value;
    use std::{fs, path::Path};

    #[test]
    fn statusline_needs_its_permission() {
        let dir = std::env::temp_dir().join(format!("wings-statusline-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |permissions: Value| {
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "permissions": permissions });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir).unwrap()
        };
        assert!(!may_read_statusline(&manifest(serde_json::json!({}))));
        assert!(!may_read_statusline(&manifest(serde_json::json!({ "transcript": ["assistant"] }))));
        assert!(may_read_statusline(&manifest(serde_json::json!({ "statusline": true }))));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ports_need_their_permission() {
        let dir = std::env::temp_dir().join(format!("wings-ports-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |permissions: Value| {
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "permissions": permissions });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir).unwrap()
        };
        assert!(!may_read_ports(&manifest(serde_json::json!({}))));
        assert!(!may_read_ports(&manifest(serde_json::json!({ "exec": ["lsof"], "statusline": true }))));
        assert!(may_read_ports(&manifest(serde_json::json!({ "ports": true }))));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sending_to_agents_needs_its_permission() {
        let dir = std::env::temp_dir().join(format!("wings-send-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |permissions: Value| {
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "permissions": permissions });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir).unwrap()
        };
        assert!(!may_send_to_agents(&manifest(serde_json::json!({ "panes": ["claude"] }))));
        assert!(may_send_to_agents(&manifest(serde_json::json!({ "sendToAgents": true }))));
        assert!(!may_create_worktrees(&manifest(serde_json::json!({ "sendToAgents": true }))));
        assert!(may_create_worktrees(&manifest(serde_json::json!({ "worktrees": true }))));
        let caller = manifest(serde_json::json!({ "callTools": ["tasks list", "changes"] }));
        assert!(may_call_tool(&caller, "tasks", "list") && may_call_tool(&caller, "changes", "recent_commits"));
        assert!(!may_call_tool(&caller, "tasks", "create") && !may_call_tool(&caller, "disk", "collect"));
        assert!(!may_call_tool(&manifest(serde_json::json!({})), "tasks", "list"));
        let hearer = manifest(serde_json::json!({ "events": ["pr-tracker pr-state"] }));
        assert!(may_hear_event(&hearer, "pr-tracker", "pr-state"));
        assert!(!may_hear_event(&hearer, "pr-tracker", "other") && !may_hear_event(&hearer, "disk", "pr-state"));
        assert!(!may_hear_event(&manifest(serde_json::json!({})), "pr-tracker", "pr-state"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn agent_activity_needs_its_permission() {
        let dir = std::env::temp_dir().join(format!("wings-activity-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |permissions: Value| {
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "permissions": permissions });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir).unwrap()
        };
        assert!(!may_read_agent_activity(&manifest(serde_json::json!({}))));
        assert!(!may_read_agent_activity(&manifest(serde_json::json!({ "transcript": ["assistant"], "statusline": true }))));
        assert!(may_read_agent_activity(&manifest(serde_json::json!({ "agentActivity": true }))));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn disk_usage_needs_its_permission() {
        let dir = std::env::temp_dir().join(format!("wings-diskusage-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.js"), "").unwrap();
        let manifest = |permissions: Value| {
            let m = serde_json::json!({ "id": "t", "name": "T", "version": "1", "api": 1, "main": "main.js", "permissions": permissions });
            fs::write(dir.join("wings-plugin.json"), m.to_string()).unwrap();
            load(&dir).unwrap()
        };
        assert!(!may_scan_disks(&manifest(serde_json::json!({}))));
        assert!(!may_scan_disks(&manifest(serde_json::json!({ "exec": ["df"], "ports": true }))));
        assert!(may_scan_disks(&manifest(serde_json::json!({ "diskUsage": true }))));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn notifications_need_permission_are_cut_to_length_and_rate_limited() {
        let mut p = plugin(Path::new("/tmp"), &[], &[]);
        assert!(notification(&p, "Done", "").unwrap_err().contains("may not show notifications"));
        p.manifest.permissions.notify = true;
        assert_eq!(notification(&p, " Build passed ", "main is green").unwrap(), ("T: Build passed".into(), "main is green".into()));
        assert!(notification(&p, "  ", "body").unwrap_err().contains("needs a title"));
        let (title, body) = notification(&p, &"t".repeat(500), &"ü".repeat(5000)).unwrap();
        assert_eq!((title.chars().count(), body.chars().count()), (3 + NOTIFY_TITLE_MAX, NOTIFY_BODY_MAX));
        assert!(body.ends_with('…'));

        let mut log = NotifyLog::default();
        let start = Instant::now();
        for i in 0..NOTIFY_LIMIT as u64 {
            assert!(log.allow("a", start + Duration::from_secs(i)).is_ok());
        }
        assert_eq!(log.allow("a", start + Duration::from_secs(10)), Err(Duration::from_secs(50)));
        // Each plugin has its own budget, and a refused call doesn't use any.
        assert!(log.allow("b", start + Duration::from_secs(10)).is_ok());
        assert!(log.allow("a", start + Duration::from_secs(59)).is_err());
        assert!(log.allow("a", start + Duration::from_secs(60)).is_ok());
        assert!(log.allow("a", start + Duration::from_secs(60)).is_err());
    }
}
