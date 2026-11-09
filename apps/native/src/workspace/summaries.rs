//! Session names and agent summaries from a model on this Mac (`wings_agents::ai`, which the desktop uses too):
//! Settings, Agents' "Session names and summaries", the desktop's `AiSettings`, and the thread that runs the model.
//! The choice and the names made so far are the desktop's `ai/` folder, so switching apps keeps both.

use std::sync::Arc;

use gpui::{AnyElement, Context, SharedString};
use wings_agents::ai::{Ai, Engine, Status};

use crate::ui;

use super::{
    Workspace, history,
    settings::{Choice, Choose},
};

/// What Settings shows: the choice and whether it works, read off the window's thread.
#[derive(Default)]
pub(super) struct View {
    /// `None` until the first check is back.
    status: Option<Status>,
    task: Option<gpui::Task<()>>,
}

/// Starts the thread that makes names and summaries from `sessions`. It waits on `ai`'s queue, so it costs nothing while
/// nothing is asked, and lives as long as the app.
pub(super) fn start(ai: Arc<Ai>, sessions: history::Sessions) {
    let spawned = std::thread::Builder::new()
        .name("ai".into())
        .spawn(move || {
            loop {
                if let Err(error) = sessions.ai_work(&ai)
                    && cfg!(debug_assertions)
                {
                    eprintln!("[ai] {error}");
                }
            }
        });
    if let Err(error) = spawned {
        eprintln!("Couldn't start making session names and summaries: {error}");
    }
}

const ENGINES: [(Engine, &str); 3] = [
    (Engine::Apple, "On-device (Apple)"),
    (Engine::Ollama, "Ollama"),
    (Engine::Off, "Off"),
];

impl Workspace {
    /// Reads the choice and checks it works. The check can build the on-device helper or ask Ollama, so it runs off the
    /// window's thread, when Agents opens.
    pub(super) fn load_ai_status(&mut self, cx: &mut Context<Self>) {
        let Some(ai) = self.ai.clone() else { return };
        self.run_ai(cx, move || ai.status());
    }

    /// Saves a new choice, as the desktop's `choose`: Ollama with no model yet takes the first one installed, so the
    /// model picker never starts empty.
    fn set_ai(&mut self, engine: Engine, model: Option<String>, cx: &mut Context<Self>) {
        let Some(ai) = self.ai.clone() else { return };
        self.run_ai(cx, move || {
            ai.set(wings_agents::ai::Settings {
                engine,
                ollama_model: model,
            });
            let status = ai.status();
            match status.models.first() {
                Some(first) if status.engine == Engine::Ollama && status.ollama_model.is_none() => {
                    ai.set(wings_agents::ai::Settings {
                        engine,
                        ollama_model: Some(first.clone()),
                    });
                    ai.status()
                }
                _ => status,
            }
        });
    }

    fn run_ai(&mut self, cx: &mut Context<Self>, work: impl FnOnce() -> Status + Send + 'static) {
        let work = cx.background_executor().spawn(async move { work() });
        self.ai_view.task = Some(cx.spawn(async move |this, cx| {
            let status = work.await;
            let _ = this.update(cx, |this, cx| {
                this.ai_view.status = Some(status);
                cx.notify();
            });
        }));
    }

    /// Shows this state instead of checking yours, for screenshots and tests.
    #[cfg(feature = "test-support")]
    pub fn show_ai_status(&mut self, status: Status, cx: &mut Context<Self>) {
        self.ai_view.status = Some(status);
        cx.notify();
    }

    /// Gives the agents these summaries, in the sidebar's order of panes, for screenshots. The next scan replaces them.
    #[cfg(feature = "test-support")]
    pub fn show_agent_summaries(&mut self, summaries: &[&str], cx: &mut Context<Self>) {
        let mut panes: Vec<usize> = self.agents.keys().copied().collect();
        panes.sort();
        for (pane, summary) in panes.into_iter().zip(summaries) {
            if let Some(agent) = self.agents.get_mut(&pane) {
                agent.summary = Some((*summary).to_owned());
            }
        }
        cx.notify();
    }

    /// The card, or nothing in an app with no model folder and nothing to show, as in tests.
    pub(super) fn summaries_card(&self, cx: &mut Context<Self>) -> Option<Vec<AnyElement>> {
        let view = &self.ai_view;
        if self.ai.is_none() && view.status.is_none() {
            return None;
        }
        let status = view.status.as_ref();
        let engine = status.map_or(Engine::Apple, |s| s.engine);
        let note = match status {
            None => Some("Checking…".to_owned()),
            Some(s) if !s.available => s
                .reason
                .as_ref()
                .map(|why| format!("{why}. Names and summaries stay off until it works.")),
            Some(_) => None,
        };
        let model = status.and_then(|s| s.ollama_model.clone());
        let engines: Vec<Choice> = ENGINES
            .into_iter()
            .map(|(choice, label)| {
                let model = model.clone();
                let choose: Choose =
                    Box::new(move |this, cx| this.set_ai(choice, model.clone(), cx));
                (label.into(), choice == engine, choose)
            })
            .collect();
        let mut rows = vec![ui::setting_row(
            "Session names and summaries",
            "Names untitled sessions and sums up each agent's last turn, from your prompt and its reply, with a \
             model on this Mac.",
            note.map(SharedString::from),
            self.select("ai-engine", "Session names and summaries", engines, cx),
        )];
        if let Some(status) = status.filter(|s| s.engine == Engine::Ollama && !s.models.is_empty())
        {
            let chosen = status.ollama_model.as_ref().unwrap_or(&status.models[0]);
            let models: Vec<Choice> = status
                .models
                .iter()
                .map(|name| {
                    let pick = name.clone();
                    let choose: Choose = Box::new(move |this, cx| {
                        this.set_ai(Engine::Ollama, Some(pick.clone()), cx)
                    });
                    (name.clone().into(), name == chosen, choose)
                })
                .collect();
            rows.push(ui::setting_row(
                "Ollama model",
                "Any model you have pulled. A small one is plenty.",
                None,
                self.select("ai-model", "Ollama model", models, cx),
            ));
        }
        Some(rows)
    }
}

#[cfg(test)]
impl Workspace {
    /// The engine Settings shows, once read.
    pub(super) fn ai_view_engine(&self) -> Option<Engine> {
        self.ai_view.status.as_ref().map(|s| s.engine)
    }
}
