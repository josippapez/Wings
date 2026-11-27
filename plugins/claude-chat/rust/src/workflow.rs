//! A workflow run as a tree in the side panel, in place of one flat line: the run, its phases, and each agent of a phase
//! with how it stands; and any background task under the subagent that started it.
//!
//! Claude Code reports a run as one background task of type `local_workflow`, whose `task_progress` lines carry
//! `workflow_progress`, the rows the terminal's `/workflows` view draws: `workflow_phase` (`index`, `title`) and
//! `workflow_agent` (`index`, `label`, `phaseIndex`, `state`, `tokens`, `toolCalls`, `error`). An agent of a workflow is not a
//! task of its own, so it has no `parent_task_id`: that field names the subagent that started a subagent, a command or a
//! monitor, which is the other half of the tree.

use std::collections::HashMap;

use serde_json::Value;
use wings_plugin::ui::{Entry, Node, entry, muted, row, spinner, stack, text};

use crate::session::{Session, Task};

/// The most runs kept: each holds a row per agent, and a deep research run starts dozens.
const RUNS_KEPT: usize = 4;
/// The most agents listed under one phase: those working or failed first, then the newest that are done.
const AGENTS_SHOWN: usize = 6;
/// How far one level moves a row in, in points.
const STEP: u32 = 16;
/// The deepest level drawn: a subagent's subagent's command is already three in.
const DEEPEST: usize = 6;

/// How an agent of a run stands, as the run's `state` says it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stand {
    /// Waiting for a place to run (`start`).
    Queued,
    Running,
    Done,
    Failed,
}

impl Stand {
    fn of(state: &str) -> Stand {
        match state {
            "done" => Stand::Done,
            "error" => Stand::Failed,
            "start" | "queued" => Stand::Queued,
            _ => Stand::Running,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Agent {
    pub index: i64,
    pub label: String,
    /// The phase it is in, by its index.
    pub phase: Option<i64>,
    pub stand: Stand,
    pub tokens: u64,
    pub tool_calls: u64,
    pub error: Option<String>,
}

/// One run: its phases in order and its agents.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Flow {
    pub phases: Vec<(i64, String)>,
    pub agents: Vec<Agent>,
}

impl Flow {
    fn of(rows: &Value) -> Flow {
        let mut flow = Flow::default();
        for row in rows.as_array().into_iter().flatten() {
            let index = row["index"].as_i64().unwrap_or(-1);
            match row["type"].as_str().unwrap_or_default() {
                "workflow_phase" => {
                    if !flow.phases.iter().any(|(known, _)| *known == index) {
                        flow.phases.push((index, row["title"].as_str().unwrap_or_default().to_owned()));
                    }
                }
                "workflow_agent" => {
                    let agent = Agent {
                        index,
                        label: row["label"].as_str().unwrap_or_default().to_owned(),
                        phase: row["phaseIndex"].as_i64(),
                        stand: Stand::of(row["state"].as_str().unwrap_or_default()),
                        tokens: row["tokens"].as_u64().unwrap_or(0),
                        tool_calls: row["toolCalls"].as_u64().unwrap_or(0),
                        error: row["error"].as_str().map(str::to_owned),
                    };
                    // The rows keep each agent's earlier states too: the last is how it stands.
                    match flow.agents.iter_mut().find(|known| known.index == index) {
                        Some(known) => *known = agent,
                        None => flow.agents.push(agent),
                    }
                    // A phase an agent names that has no row of its own is still a group.
                    if let (Some(phase), Some(title)) = (row["phaseIndex"].as_i64(), row["phaseTitle"].as_str())
                        && !flow.phases.iter().any(|(known, _)| *known == phase)
                    {
                        flow.phases.push((phase, title.to_owned()));
                    }
                }
                _ => {}
            }
        }
        flow.phases.sort_by_key(|(index, _)| *index);
        flow
    }

    fn count(&self, phase: Option<i64>, stand: Stand) -> usize {
        self.agents.iter().filter(|agent| agent.phase == phase && agent.stand == stand).count()
    }
}

/// What the session heard of how its background tasks nest.
#[derive(Debug, Default, PartialEq)]
pub struct Flows {
    /// The subagent each live task was started by, by task id (`parent_task_id`).
    parents: HashMap<String, String>,
    /// The runs of workflows, by the id of the task that runs each, the oldest first.
    runs: Vec<(String, Flow)>,
}

impl Flows {
    /// Takes in a line Claude Code printed.
    pub fn heard(&mut self, line: &Value) {
        if line["type"] != "system" {
            return;
        }
        match line["subtype"].as_str().unwrap_or_default() {
            // The whole live set: a task that left it has no parent to show.
            "background_tasks_changed" => {
                self.parents = line["tasks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|task| Some((task["task_id"].as_str()?.to_owned(), task["parent_task_id"].as_str().filter(|parent| !parent.is_empty())?.to_owned())))
                    .collect();
            }
            "task_started" => {
                if let (Some(id), Some(parent)) = (line["task_id"].as_str(), line["parent_task_id"].as_str().filter(|parent| !parent.is_empty())) {
                    self.parents.insert(id.to_owned(), parent.to_owned());
                }
            }
            "task_progress" if line["workflow_progress"].is_array() => {
                let Some(id) = line["task_id"].as_str() else {
                    return;
                };
                let flow = Flow::of(&line["workflow_progress"]);
                match self.runs.iter_mut().find(|(known, _)| known == id) {
                    Some((_, known)) => *known = flow,
                    None => {
                        self.runs.push((id.to_owned(), flow));
                        let extra = self.runs.len().saturating_sub(RUNS_KEPT);
                        self.runs.drain(..extra);
                    }
                }
            }
            _ => {}
        }
    }

    /// Whether a workflow has run in this session.
    pub fn any(&self) -> bool {
        !self.runs.is_empty()
    }

    pub fn flow(&self, task: &str) -> Option<&Flow> {
        self.runs.iter().find(|(id, _)| id == task).map(|(_, flow)| flow)
    }

    fn parent(&self, task: &str) -> Option<&str> {
        self.parents.get(task).map(String::as_str)
    }
}

/// The running tasks as a tree: each one that no running task started, then under it the run's phases and agents when it is
/// a workflow, and the tasks it started, each drawn by `draw` and moved in a step for each level.
pub fn tree(session: &Session, running: &[&Task], draw: &dyn Fn(&Task) -> Node) -> Vec<Node> {
    let mut rows = Vec::new();
    let mut placed: Vec<&str> = Vec::new();
    let roots = running.iter().filter(|task| session.flows.parent(&task.id).is_none_or(|parent| running.iter().all(|other| other.id != parent)));
    for task in roots {
        place(session, running, task, 0, draw, &mut placed, &mut rows);
    }
    rows
}

fn place<'a>(session: &Session, running: &[&'a Task], task: &'a Task, depth: usize, draw: &dyn Fn(&Task) -> Node, placed: &mut Vec<&'a str>, rows: &mut Vec<Node>) {
    if placed.contains(&task.id.as_str()) || depth > DEEPEST {
        return;
    }
    placed.push(&task.id);
    rows.push(indented(depth, draw(task)).key(format!("flow-{}", task.id)));
    if let Some(flow) = session.flows.flow(&task.id) {
        rows.extend(flow_rows(&task.id, flow, depth + 1));
    }
    for child in running.iter().filter(|child| session.flows.parent(&child.id) == Some(task.id.as_str())) {
        place(session, running, child, depth + 1, draw, placed, rows);
    }
}

/// `node` moved in a step for each level, by a spacer before it.
fn indented(depth: usize, node: Node) -> Node {
    if depth == 0 {
        return node;
    }
    row().prop("gap", "none").child(stack().prop("width", depth as u32 * STEP)).child(stack().prop("grow", true).child(node))
}

/// The phases of a run, each with its agents.
fn flow_rows(task: &str, flow: &Flow, depth: usize) -> Vec<Node> {
    let mut rows = Vec::new();
    let mut groups: Vec<(Option<i64>, String)> = flow.phases.iter().map(|(index, title)| (Some(*index), title.clone())).collect();
    // Agents of no phase come first, under no heading.
    if flow.agents.iter().any(|agent| agent.phase.is_none_or(|phase| flow.phases.iter().all(|(index, _)| *index != phase))) {
        groups.insert(0, (None, String::new()));
    }
    for (phase, title) in groups {
        let own = |agent: &&Agent| match phase {
            Some(phase) => agent.phase == Some(phase),
            None => agent.phase.is_none_or(|phase| flow.phases.iter().all(|(index, _)| *index != phase)),
        };
        let members: Vec<&Agent> = flow.agents.iter().filter(own).collect();
        if phase.is_some() {
            let (done, failed) = (members.iter().filter(|agent| agent.stand == Stand::Done).count(), members.iter().filter(|agent| agent.stand == Stand::Failed).count());
            let mut said = format!("{done} of {} done", members.len());
            if failed > 0 {
                said.push_str(&format!(" · {failed} failed"));
            }
            let head = entry(Entry { lead: None, title: text(title.as_str()).prop("size", "small").prop("weight", "strong").prop("clamp", true).prop("title", title.as_str()), detail: Some(muted(said).prop("size", "small")), trailing: None });
            rows.push(indented(depth, head).key(format!("flow-{task}-phase-{}", phase.unwrap_or(-1))));
        }
        // Working and failed agents are listed first, then the newest that are done, and the list goes back in the run's order.
        let mut shown: Vec<&Agent> = members.iter().copied().filter(|agent| agent.stand != Stand::Done).collect();
        let room = AGENTS_SHOWN.saturating_sub(shown.len());
        shown.extend(members.iter().copied().filter(|agent| agent.stand == Stand::Done).rev().take(room));
        shown.truncate(AGENTS_SHOWN.max(members.iter().filter(|agent| agent.stand != Stand::Done).count()));
        shown.sort_by_key(|agent| agent.index);
        let hidden = members.len() - shown.len();
        let inner = if phase.is_some() { depth + 1 } else { depth };
        rows.extend(shown.into_iter().map(|agent| indented(inner, agent_row(task, agent))));
        if hidden > 0 {
            rows.push(indented(inner, entry(Entry { lead: None, title: muted(format!("{hidden} more done")).prop("size", "small"), detail: None, trailing: None })).key(format!("flow-{task}-more-{}", phase.unwrap_or(-1))));
        }
    }
    rows
}

/// One agent: how it stands, its label, and what it has used.
fn agent_row(task: &str, agent: &Agent) -> Node {
    let mark = match agent.stand {
        Stand::Running => spinner(),
        Stand::Done => text("✓").prop("tone", "success"),
        Stand::Failed => text("✗").prop("tone", "danger"),
        Stand::Queued => muted("·"),
    };
    let name = if agent.label.is_empty() { format!("Agent {}", agent.index + 1) } else { agent.label.clone() };
    let mut used = Vec::new();
    match agent.stand {
        Stand::Queued => used.push("Waiting".to_owned()),
        Stand::Failed => used.push(agent.error.clone().map_or_else(|| "Failed".to_owned(), |why| wings_plugin::format::truncate(&why, 60))),
        _ => {}
    }
    if agent.tool_calls > 0 {
        used.push(format!("{} tool call{}", agent.tool_calls, if agent.tool_calls == 1 { "" } else { "s" }));
    }
    if agent.tokens > 0 {
        used.push(format!("{} tokens", wings_plugin::format::tokens(agent.tokens as f64)));
    }
    let detail = (!used.is_empty()).then(|| muted(used.join(" · ")).prop("size", "small").prop("clamp", true));
    entry(Entry { lead: Some(mark), title: text(name.as_str()).prop("size", "small").prop("clamp", true).prop("title", name.as_str()), detail, trailing: None }).key(format!("flow-{task}-agent-{}", agent.index))
}
