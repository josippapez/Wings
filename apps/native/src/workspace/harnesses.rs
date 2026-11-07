//! Settings, Agents' "Coding agents", the desktop's `Harnesses`: the agent CLIs on this Mac (`wings_connect::harnesses`),
//! each one's version and whether it's signed in, with Install, Sign in and its docs. Each check runs the agent's own CLI,
//! so they run when the section opens and on Refresh, off the window's thread, never on a timer or at startup.

use gpui::{
    AnyElement, Context, FontWeight, Role, SharedString, Window, div, prelude::*, px, rgb, rgba,
};
use wings_connect::harnesses::{self, Auth, HarnessStatus, Lookup};

use crate::{
    icons::{Icon, icon},
    page::plugin::OpenPane,
    page::runtime::Reply,
    theme,
    ui::{self, ButtonSize},
};

use super::Workspace;

/// The checks as the section shows them.
#[derive(Default)]
pub(super) struct Checks {
    /// `None` until the first check is back.
    list: Option<Vec<HarnessStatus>>,
    checking: bool,
    error: Option<String>,
    task: Option<gpui::Task<()>>,
}

/// What a row says under the name, and its dot's color, as the desktop's `state`.
fn state(h: &HarnessStatus) -> (String, u32) {
    if !h.installed {
        return ("Not installed".into(), theme::IDLE);
    }
    match h.auth {
        Auth::SignedIn => (
            h.account
                .as_ref()
                .map_or("Signed in".into(), |a| format!("Signed in · {a}")),
            theme::DONE,
        ),
        Auth::SignedOut => ("Not signed in".into(), theme::WORKING),
        Auth::Unknown => (
            h.note.clone().unwrap_or_else(|| "Installed".into()),
            theme::IDLE,
        ),
    }
}

fn glyph(h: &HarnessStatus) -> AnyElement {
    let mark = match h.id.as_str() {
        "claude" => Some((Icon::Claude, 0xd97757)),
        "opencode" => Some((Icon::Opencode, theme::FOREGROUND)),
        "codex" => Some((Icon::Codex, theme::FOREGROUND)),
        "cursor" => Some((Icon::Cursor, theme::FOREGROUND)),
        "grok" => Some((Icon::Grok, theme::FOREGROUND)),
        _ => None,
    };
    match mark {
        Some((glyph, color)) => icon(glyph, 16.0, color).into_any_element(),
        None => div()
            .size(px(16.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.0))
            .bg(rgba(0xffffff1a))
            .text_size(px(10.0))
            .font_weight(FontWeight::SEMIBOLD)
            .child(h.name.chars().next().unwrap_or('?').to_string())
            .into_any_element(),
    }
}

impl Workspace {
    /// Checks every agent CLI again. Only an app that serves the socket runs yours, as with Claude Code's connection,
    /// so tests and screenshots run none.
    pub(super) fn load_harnesses(&mut self, cx: &mut Context<Self>) {
        if self.config.socket.is_none() || self.harnesses.checking {
            return;
        }
        self.harnesses.checking = true;
        self.harnesses.error = None;
        cx.notify();
        let work = cx
            .background_executor()
            .spawn(async move { harnesses::check_all(&Lookup::system()) });
        self.harnesses.task = Some(cx.spawn(async move |this, cx| {
            let list = work.await;
            let _ = this.update(cx, |this, cx| {
                this.harnesses.checking = false;
                this.harnesses.list = Some(list);
                cx.notify();
            });
        }));
    }

    /// Shows these agents instead of checking yours, for screenshots.
    #[cfg(feature = "test-support")]
    pub fn show_harnesses(&mut self, list: Vec<HarnessStatus>, cx: &mut Context<Self>) {
        self.harnesses.list = Some(list);
        cx.notify();
    }

    /// A new tab in the project on screen with `input` typed into it, as the desktop's `onRunInTab`.
    fn run_in_tab(&mut self, input: String, window: &mut Window, cx: &mut Context<Self>) {
        self.close_settings(window, cx);
        let project = self.config.project.clone();
        let plan = OpenPane {
            cwd: project.clone(),
            project,
            input: Some(input),
            placement: "tab".into(),
            claude_account: None,
        };
        let (reply, _answer) = Reply::channel();
        self.open_plugin_pane(plan, reply, window, cx);
    }

    /// The card: a header with Refresh, then a row for each agent.
    pub(super) fn harnesses_card(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let checks = &self.harnesses;
        let refresh = ui::button(
            "harnesses-refresh",
            "Check the coding agents again",
            ui::Variant::Ghost,
            checks.checking,
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(icon(Icon::RefreshCw, 14.0, theme::MUTED_FOREGROUND))
                .child(if checks.checking {
                    "Checking…"
                } else {
                    "Refresh"
                }),
        )
        .px(px(8.0))
        .when(!checks.checking, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.load_harnesses(cx)))
        });
        let mut rows = vec![ui::setting_row(
            "Coding agents",
            "The agent CLIs on this Mac, and whether each is signed in.",
            checks
                .error
                .clone()
                .map(|e| SharedString::from(format!("Couldn't check the agents: {e}"))),
            refresh,
        )];
        match &checks.list {
            None => rows.extend((0..5).map(|_| placeholder())),
            Some(list) => rows.extend(list.iter().map(|h| self.harness_row(h, cx))),
        }
        rows
    }

    fn harness_row(&self, h: &HarnessStatus, cx: &mut Context<Self>) -> AnyElement {
        let (text, tone) = state(h);
        let mut actions = div().flex().flex_none().items_center().gap(px(4.0));
        if !h.installed {
            let docs = h.docs.clone();
            let install = h.install.clone();
            actions = actions
                .child(
                    ui::icon_button(
                        SharedString::from(format!("harness-docs-{}", h.id)),
                        Icon::ExternalLink,
                        format!("How to install {}", h.name),
                        ButtonSize::Sm,
                    )
                    .on_click(move |_, _, cx| cx.open_url(&docs)),
                )
                // Typed but not run: you read the command and press Enter.
                .child(
                    ui::button(
                        SharedString::from(format!("harness-install-{}", h.id)),
                        format!(
                            "Install {}: types {install} into a new tab, for you to run",
                            h.name
                        ),
                        ui::Variant::Secondary,
                        false,
                        "Install",
                    )
                    .h(px(28.0))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run_in_tab(install.clone(), window, cx)
                    })),
                );
        }
        if h.installed && h.auth == Auth::SignedOut {
            let login = h.login.clone();
            actions = actions.child(
                ui::button(
                    SharedString::from(format!("harness-login-{}", h.id)),
                    format!("Sign in to {}: runs {login} in a new tab", h.name),
                    ui::Variant::Secondary,
                    false,
                    "Sign in",
                )
                .h(px(28.0))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.run_in_tab(format!("{login}\r"), window, cx)
                })),
            );
        }
        div()
            .id(SharedString::from(format!("harness-{}", h.id)))
            .role(Role::Group)
            .aria_label(format!("{}: {text}", h.name))
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(16.0))
            .py(px(12.0))
            .child(glyph(h))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(8.0))
                            .text_size(px(13.0))
                            .child(h.name.clone())
                            .when_some(h.version.clone(), |name, version| {
                                name.child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .font_family(crate::terminal_view::DEFAULT_FONT_FAMILY)
                                        .text_size(px(11.0))
                                        .text_color(rgb(theme::MUTED_FOREGROUND))
                                        .child(version),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .text_size(px(12.0))
                            .text_color(rgb(theme::MUTED_FOREGROUND))
                            .child(ui::dot(tone))
                            .child(div().min_w_0().truncate().child(text)),
                    ),
            )
            .child(actions)
            .into_any_element()
    }
}

/// A grey row while the first check runs, as the desktop's skeletons.
fn placeholder() -> AnyElement {
    let bar = |w: f32, h: f32, alpha: u32| {
        div()
            .w(px(w))
            .h(px(h))
            .rounded(px(4.0))
            .bg(rgba(0xffffff00 | alpha))
    };
    div()
        .flex()
        .items_center()
        .gap(px(12.0))
        .px(px(16.0))
        .py(px(12.0))
        .child(bar(16.0, 16.0, 0x0f))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(bar(112.0, 14.0, 0x0f))
                .child(bar(160.0, 12.0, 0x0a)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(
        installed: bool,
        auth: Auth,
        account: Option<&str>,
        note: Option<&str>,
    ) -> HarnessStatus {
        HarnessStatus {
            id: "codex".into(),
            name: "Codex".into(),
            installed,
            version: None,
            auth,
            account: account.map(Into::into),
            note: note.map(Into::into),
            login: "codex login".into(),
            install: "x".into(),
            docs: "y".into(),
        }
    }

    #[test]
    fn each_row_says_what_the_desktop_says() {
        assert_eq!(
            state(&status(false, Auth::Unknown, None, None)),
            ("Not installed".into(), theme::IDLE)
        );
        assert_eq!(
            state(&status(true, Auth::SignedIn, Some("ChatGPT"), None)),
            ("Signed in · ChatGPT".into(), theme::DONE)
        );
        assert_eq!(
            state(&status(true, Auth::SignedIn, None, None)).0,
            "Signed in"
        );
        assert_eq!(
            state(&status(true, Auth::SignedOut, None, None)),
            ("Not signed in".into(), theme::WORKING)
        );
        assert_eq!(
            state(&status(
                true,
                Auth::Unknown,
                None,
                Some("codex didn't answer in 8s")
            ))
            .0,
            "codex didn't answer in 8s"
        );
    }

    /// Install and Sign in leave Settings for a new tab in the project on screen, with the command typed into it.
    #[cfg(feature = "test-support")]
    #[test]
    fn install_and_sign_in_open_a_tab_in_the_project_on_screen() {
        let (_app, mut window) = super::super::tests::open_with(None);
        let tabs = window.read(|w, _| w.terminals.len());
        window.update(|w, window, cx| {
            w.open_settings(window, cx);
            w.run_in_tab("codex login\r".into(), window, cx);
        });
        window.read(|w, _| {
            assert!(w.settings.is_none());
            assert_eq!(w.terminals.len(), tabs + 1);
            let new = w.terminals.last().unwrap();
            assert_eq!(w.active, super::super::Active::Terminal(new.id));
        });
    }
}
