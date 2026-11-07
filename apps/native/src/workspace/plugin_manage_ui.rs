//! Settings, Plugins and the dialog that asks before a plugin runs, laid out like the desktop's `plugins-section.tsx`:
//! install from a GitHub link or a file at the top, the installed plugins below, and the selected one's page under its
//! row. The desktop shows the list and the page side by side; Settings here is too narrow for two columns.

use gpui::{
    AnyElement, Context, ExternalPaths, FontWeight, KeyDownEvent, MouseButton, Role, SharedString,
    deferred, div, prelude::*, px, rgb, rgba,
};
use wings_plugins::{
    manifest::Manifest,
    store::{PluginView, Source},
};

use crate::{
    icons::{Icon, icon},
    theme,
    ui::{self, Variant},
};

use super::Workspace;

/// The desktop's names for a plugin's `ui` parts.
fn ui_name(kind: &str) -> String {
    match kind {
        "badges" => "Pane badges".into(),
        "diff" => "Diff viewer".into(),
        other => other.into(),
    }
}

/// The host of an https address, as the desktop's `new URL(url).host`.
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', '?', '#'])
        .next()
        .unwrap_or(rest)
        .to_owned()
}

/// What turning a plugin on lets it do, in plain words, as the desktop's `Permissions` lists it: each group's title
/// and its items. A yes or no permission that is on is a group without items.
pub(super) fn access_groups(manifest: &Manifest) -> Vec<(&'static str, Vec<String>)> {
    let p = &manifest.permissions;
    let c = &manifest.contributes;
    let shows: Vec<String> =
        c.ui.iter()
            .map(|kind| ui_name(kind))
            .chain(c.sidebars.iter().map(|s| {
                format!(
                    "{} sidebar{}",
                    s.title,
                    if s.popover.is_some() {
                        " and popover"
                    } else {
                        ""
                    }
                )
            }))
            .chain(
                c.views
                    .iter()
                    .map(|v| format!("{} {}", v.title, v.opens.join(" or "))),
            )
            .chain(c.settings.as_ref().map(|_| "Settings page".to_owned()))
            .collect();
    let on = |yes: bool| if yes { Some(Vec::new()) } else { None };
    let list = |items: &[String]| (!items.is_empty()).then(|| items.to_vec());
    [
        ("Show these in Wings", list(&shows)),
        ("Call these web addresses, with tokens you give it", list(&p.fetch)),
        (
            "Keep these tokens in your keychain, each sent only to its own address",
            list(&p.secrets.iter().map(|(name, prefix)| format!("{name} → {prefix}")).collect::<Vec<_>>()),
        ),
        ("Open these sites in a panel from the title bar", list(&c.panels.iter().map(|panel| host_of(&panel.url)).collect::<Vec<_>>())),
        ("Offer Claude these tools over MCP", list(&c.mcp_tools.iter().map(|tool| tool.name.clone()).collect::<Vec<_>>())),
        ("Answer these Claude Code hooks: add context to a session, or refuse a Bash command", list(&c.hooks)),
        ("Run these commands as you, with your own sign-ins", list(&p.exec)),
        ("Post to these addresses as you, like replying to a review comment", list(&p.post)),
        ("Open terminals in your projects and move your typing to any pane", on(p.panes.is_some())),
        ("Start these commands in a new terminal, as you", p.panes.as_deref().and_then(list)),
        ("Show desktop notifications, up to 20 a minute", on(p.notify)),
        ("Type messages into the prompts of the agents in your terminals", on(p.send_to_agents)),
        ("Make git worktrees of your projects, on new branches beside the repo", on(p.worktrees)),
        (
            "Choose which Claude Code account each project's new terminals use, and see the email each one is signed in with",
            on(p.claude_accounts),
        ),
        ("Use these Wings plugins' tools and data", list(&p.call_tools)),
        ("Hear these events from other Wings plugins", list(&p.events)),
        ("Read these parts of your Claude Code sessions", list(&p.transcript)),
        ("Read your Claude usage limits and each session's context and cache, from Claude Code's status line", on(p.statusline)),
        ("Read what your OpenCode sessions are doing: their latest prompts, tool calls and token use", on(p.agent_activity)),
        ("See which local ports the programs in your terminals listen on", on(p.ports)),
        ("Read the names and sizes of the files and folders on your disks", on(p.disk_usage)),
        ("Open links that start with", list(&p.open_url)),
    ]
    .into_iter()
    .filter_map(|(title, items)| Some((title, items?)))
    .collect()
}

fn muted(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_size(px(12.0))
        .text_color(rgb(theme::MUTED_FOREGROUND))
        .child(text.into())
}

/// The groups of `access_groups`, each a title over its items as small code chips.
fn access_list(manifest: &Manifest) -> AnyElement {
    let groups = access_groups(manifest);
    if groups.is_empty() {
        return muted("It doesn't ask for any access beyond showing things in Wings.")
            .into_any_element();
    }
    div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .children(groups.into_iter().map(|(title, items)| {
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(muted(title).font_weight(FontWeight::MEDIUM))
                .when(!items.is_empty(), |group| {
                    group.child(div().flex().flex_wrap().gap(px(6.0)).children(
                        items.into_iter().map(|item| {
                            // The desktop's `max-w-full break-all`: a long command wraps inside the page.
                            div()
                                .max_w_full()
                                .px(px(6.0))
                                .py(px(2.0))
                                .rounded(px(6.0))
                                .bg(rgba(0xffffff0f))
                                .font_family("Menlo")
                                .text_size(px(12.0))
                                .child(item)
                        }),
                    ))
                })
        }))
        .into_any_element()
}

/// Where the plugin came from, as the desktop's `Source` says it.
fn source_line(view: &PluginView) -> String {
    if view.dev {
        return "From this repo (development build)".into();
    }
    if let Some(folder) = &view.folder {
        return format!("Linked from {}", folder.display());
    }
    match &view.source {
        Some(Source::Github {
            repo,
            folder: Some(folder),
        }) => format!("From GitHub, {repo}/{folder}"),
        Some(Source::Github { repo, folder: None }) => format!("From GitHub, {repo}"),
        _ => "From a file".into(),
    }
}

fn plugin_icon(on: bool, side: f32) -> gpui::Div {
    div()
        .size(px(side))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.0))
        .bg(rgba(if on { 0xb392f026 } else { 0xffffff0f }))
        .child(icon(
            Icon::Puzzle,
            side / 2.0,
            if on {
                0xc9b2f7
            } else {
                theme::MUTED_FOREGROUND
            },
        ))
}

impl Workspace {
    /// The status under a plugin's name: the desktop's, and for one that is on whether it runs here.
    pub(super) fn plugin_status(&self, view: &PluginView) -> String {
        let state = if !view.approved {
            "Waiting for approval"
        } else if view.enabled {
            "On"
        } else {
            "Off"
        };
        match (
            view.enabled && view.approved,
            self.plugin_runs(&view.manifest.id),
        ) {
            (true, true) => format!("{state} · Runs here"),
            (true, false) => format!("{state} · No native build"),
            _ => state.to_owned(),
        }
    }

    pub(super) fn plugins_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(dialog) = &self.settings else {
            return div().into_any_element();
        };
        let manage = &self.manage;
        let typed = !dialog.link.read(cx).text.trim().is_empty();
        let installing = manage.installing.is_some();
        let install =
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .px(px(16.0))
                .py(px(12.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(ui::input(dialog.link.clone())),
                        )
                        .child(
                            ui::button(
                                "plugin-install-link",
                                "Install",
                                Variant::Default,
                                !typed || installing,
                                if manage.installing == Some("link") {
                                    "Installing…"
                                } else {
                                    "Install"
                                },
                            )
                            .when(typed && !installing, |button| {
                                button.on_click(cx.listener(|this, _, window, cx| {
                                    this.install_from_link(window, cx)
                                }))
                            }),
                        )
                        .child(
                            ui::button(
                                "plugin-install-file",
                                "Choose a file",
                                Variant::Outline,
                                installing,
                                if manage.installing == Some("file") {
                                    "Installing…"
                                } else {
                                    "Choose a file"
                                },
                            )
                            .when(!installing, |button| {
                                button.on_click(cx.listener(|this, _, window, cx| {
                                    this.install_from_file(window, cx)
                                }))
                            }),
                        ),
                )
                .child(muted(
                    "A GitHub repo or plugin folder link, or a .wings-plugin file.",
                ))
                .child(muted(
                    "You can also drop a .wings-plugin file on the window.",
                ))
                .when_some(manage.error.clone(), |column, error| {
                    column.child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(theme::DANGER))
                            .child(error),
                    )
                });

        let views = self.plugin_views();
        let updatable: Vec<String> = views
            .iter()
            .filter(|view| manage.updates.contains_key(&view.manifest.id))
            .map(|view| view.manifest.id.clone())
            .collect();
        let updating = updatable.iter().any(|id| manage.busy.contains(id));
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .child(muted("Installed").font_weight(FontWeight::MEDIUM))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(
                        ui::button(
                            "plugin-check-updates",
                            "Check for updates",
                            Variant::Ghost,
                            manage.checking,
                            if manage.checking {
                                "Checking…"
                            } else {
                                "Check for updates"
                            },
                        )
                        .children(ui::spot(|| "plugin-check-updates".into()))
                        .when(!manage.checking, |button| {
                            button.on_click(
                                cx.listener(|this, _, _, cx| this.check_plugin_updates(true, cx)),
                            )
                        }),
                    )
                    .when(!updatable.is_empty(), |row| {
                        let label = format!("Update all ({})", updatable.len());
                        row.child(
                            ui::button(
                                "plugin-update-all",
                                label.clone(),
                                Variant::Ghost,
                                updating,
                                label,
                            )
                            .when(!updating, |button| {
                                button.on_click(cx.listener(move |this, _, window, cx| {
                                    this.update_plugins(updatable.clone(), window, cx)
                                }))
                            }),
                        )
                    }),
            );

        let available: Vec<_> = manage
            .available
            .iter()
            .filter(|plugin| !views.iter().any(|view| view.manifest.id == plugin.id))
            .cloned()
            .collect();
        let selected = manage.selected.clone().or_else(|| {
            views
                .first()
                .filter(|_| !manage.browsing_available)
                .map(|view| view.manifest.id.clone())
        });
        let rows: Vec<AnyElement> = views
            .into_iter()
            .enumerate()
            .map(|(index, view)| {
                let open = selected.as_deref() == Some(view.manifest.id.as_str());
                self.plugin_row(index, view, open, cx)
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(muted("Plugins add features to Wings. Each one runs only after you approve what it can do."))
            .child(ui::card([install.into_any_element()]))
            .child(header)
            .child(if rows.is_empty() {
                ui::card([muted("No plugins yet. Paste a GitHub link or drop a .wings-plugin file to add one.")
                    .px(px(16.0))
                    .py(px(16.0))
                    .into_any_element()])
            } else {
                ui::card(rows)
            })
            .when(!available.is_empty(), |column| {
                column.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .child(muted("Available").font_weight(FontWeight::MEDIUM))
                        .child(ui::card(available.into_iter().enumerate().map(|(index, plugin)| {
                            let label = format!("Install {}", plugin.name);
                            let link = plugin.link.clone();
                            div()
                                .id(("plugin-available", index))
                                .flex()
                                .items_center()
                                .gap(px(12.0))
                                .px(px(16.0))
                                .py(px(10.0))
                                .child(plugin_icon(false, 32.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.0))
                                        .child(
                                            div().flex().items_baseline().gap(px(8.0))
                                                .child(div().truncate().font_weight(FontWeight::MEDIUM).child(plugin.name))
                                                .child(muted(plugin.version).text_size(px(11.0)).flex_none()),
                                        )
                                        .child(muted(plugin.description).text_size(px(12.0))),
                                )
                                .child(
                                    ui::button(("plugin-available-install", index), label, Variant::Secondary, manage.installing.is_some(), "Install")
                                        .when(manage.installing.is_none(), |button| {
                                            button.on_click(cx.listener(move |this, _, window, cx| {
                                                this.install_available(link.clone(), window, cx)
                                            }))
                                        }),
                                )
                                .into_any_element()
                        })))
                )
            })
            .into_any_element()
    }

    /// One installed plugin: its row, and its page under it while it is the selected one.
    fn plugin_row(
        &self,
        index: usize,
        view: PluginView,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let manage = &self.manage;
        let id = view.manifest.id.clone();
        let on = view.enabled && view.approved;
        let busy = manage.busy.contains(&id);
        let update = manage.updates.get(&id).cloned();
        let status = self.plugin_status(&view);
        let select = id.clone();
        let toggled = view.clone();
        let row = div()
            .id(("plugin-row", index))
            .role(Role::Button)
            .aria_label(view.manifest.name.clone())
            .aria_selected(open)
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(16.0))
            .py(px(10.0))
            .cursor_pointer()
            .hover(|style| style.bg(rgba(theme::HOVER)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.manage.selected = Some(select.clone());
                this.manage.browsing_available = false;
                this.manage.removing = None;
                cx.notify();
            }))
            .child(plugin_icon(on, 32.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(view.manifest.name.clone()),
                            )
                            .child(
                                muted(view.manifest.version.clone())
                                    .text_size(px(11.0))
                                    .flex_none(),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .min_w_0()
                            .text_size(px(12.0))
                            .child(
                                div()
                                    .truncate()
                                    .text_color(rgb(if view.approved {
                                        theme::MUTED_FOREGROUND
                                    } else {
                                        theme::WORKING
                                    }))
                                    .child(status),
                            )
                            .when(update.is_some(), |text| {
                                text.child(
                                    div()
                                        .flex_none()
                                        .text_color(rgb(theme::WORKING))
                                        .child("\u{a0}· Update available"),
                                )
                            }),
                    ),
            )
            .child(
                ui::switch(("plugin-on", index), on)
                    .aria_label(format!(
                        "{} {}",
                        view.manifest.name,
                        if on { "on" } else { "off" }
                    ))
                    .when(busy, |switch| switch.opacity(0.5))
                    .when(!busy, |switch| {
                        switch
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.toggle_plugin(toggled.clone(), !on, window, cx)
                            }))
                    }),
            );
        if !open {
            return row.into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .child(row)
            .child(self.plugin_details(index, view, cx))
            .into_any_element()
    }

    /// The selected plugin's page: where it came from, what it does, its buttons and what it can access.
    fn plugin_details(&self, index: usize, view: PluginView, cx: &mut Context<Self>) -> AnyElement {
        let manage = &self.manage;
        let id = view.manifest.id.clone();
        let busy = manage.busy.contains(&id);
        let update = manage.updates.get(&id).cloned();
        let github = matches!(view.source, Some(Source::Github { .. })) && !view.dev;
        let mut buttons = div().flex().flex_wrap().gap(px(8.0));
        if let Some(version) = update {
            let ids = vec![id.clone()];
            buttons = buttons.child(
                ui::button(
                    ("plugin-update", index),
                    format!("Update to {version}"),
                    Variant::Secondary,
                    busy,
                    format!("Update to {version}"),
                )
                .when(!busy, |button| {
                    button.on_click(cx.listener(move |this, _, window, cx| {
                        this.update_plugins(ids.clone(), window, cx)
                    }))
                }),
            );
        }
        if github {
            let ids = vec![id.clone()];
            buttons = buttons.child(
                ui::button(
                    ("plugin-reinstall", index),
                    "Reinstall from GitHub",
                    Variant::Outline,
                    busy,
                    "Reinstall from GitHub",
                )
                .when(!busy, |button| {
                    button.on_click(cx.listener(move |this, _, window, cx| {
                        this.update_plugins(ids.clone(), window, cx)
                    }))
                }),
            );
        }
        if !view.dev {
            if manage.removing.as_deref() == Some(id.as_str()) {
                let gone = id.clone();
                buttons = buttons
                    .child(
                        ui::button(
                            ("plugin-remove-confirm", index),
                            "Remove plugin",
                            Variant::Destructive,
                            busy,
                            "Remove plugin",
                        )
                        .when(!busy, |button| {
                            button.on_click(cx.listener(move |this, _, window, cx| {
                                this.remove_plugin_now(&gone, window, cx)
                            }))
                        }),
                    )
                    .child(
                        ui::button(
                            ("plugin-keep", index),
                            "Keep",
                            Variant::Ghost,
                            false,
                            "Keep",
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.manage.removing = None;
                            cx.notify();
                        })),
                    );
            } else {
                let asked = id.clone();
                buttons = buttons.child(
                    ui::button(
                        ("plugin-remove", index),
                        "Remove",
                        Variant::Outline,
                        busy,
                        div().text_color(rgb(theme::DANGER)).child("Remove"),
                    )
                    .when(!busy, |button| {
                        button.on_click(cx.listener(move |this, _, _, cx| {
                            this.manage.removing = Some(asked.clone());
                            cx.notify();
                        }))
                    }),
                );
            }
        }
        let review = view.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .px(px(16.0))
            .pt(px(4.0))
            .pb(px(16.0))
            .child(muted(source_line(&view)))
            .when(!view.manifest.description.is_empty(), |page| {
                page.child(
                    div()
                        .text_size(px(13.0))
                        .child(view.manifest.description.clone()),
                )
            })
            .when(!view.approved, |page| {
                // In a row of its own, so it is as wide as its label, as the desktop's `w-fit`.
                page.child(
                    div().flex().child(
                        ui::button(
                            ("plugin-review", index),
                            "Review access to turn it on",
                            Variant::Outline,
                            false,
                            div()
                                .text_color(rgb(theme::WORKING))
                                .child("Review access to turn it on"),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.queue_reviews([review.clone()], cx);
                            cx.notify();
                        })),
                    ),
                )
            })
            .child(buttons)
            .when_some(manage.row_error.get(&id).cloned(), |page, error| {
                page.child(
                    div()
                        .text_size(px(12.0))
                        .text_color(rgb(theme::DANGER))
                        .child(error),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("What it can access"),
                    )
                    .child(access_list(&view.manifest)),
            )
            .when_some(self.plugin_readme(&view), |page, blocks| {
                page.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .pt(px(16.0))
                        .border_t_1()
                        .border_color(rgba(theme::BORDER))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("README"),
                        )
                        .child(crate::markdown::render(
                            &format!("plugin-readme-{index}"),
                            &blocks,
                        ))
                        .children(ui::spot(|| "plugin-readme".into())),
                )
            })
            .into_any_element()
    }

    /// Over everything, where a `.wings-plugin` file can be dropped anywhere on the window, as on the desktop's. It
    /// shows only while one is dragged over the window, and lets every click and hover through.
    pub(super) fn plugin_drop_layer(&self, cx: &mut Context<Self>) -> AnyElement {
        let shown = |paths: &ExternalPaths| super::plugin_manage::carries_package(paths);
        deferred(
            div()
                .id("plugin-drop")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme::RADIUS))
                .border_2()
                .border_color(gpui::transparent_black())
                .drag_over::<ExternalPaths>(move |style, paths, _, _| {
                    if shown(paths) {
                        style.bg(rgba(0x0f0e1299)).border_color(rgba(0xe9b65fb3))
                    } else {
                        style
                    }
                })
                .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                    this.drop_package(paths, window, cx)
                }))
                .child(
                    div()
                        .id("plugin-drop-label")
                        .px(px(14.0))
                        .py(px(8.0))
                        .rounded(px(10.0))
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .bg(rgba(theme::POPOVER))
                        .text_color(rgb(theme::FOREGROUND))
                        // A drag over only refines what paints, not the text's color, so the label fades in whole.
                        .opacity(0.0)
                        .drag_over::<ExternalPaths>(move |style, paths, _, _| {
                            if shown(paths) {
                                style.opacity(1.0)
                            } else {
                                style
                            }
                        })
                        .child("Drop to install the plugin"),
                ),
        )
        .with_priority(5)
        .into_any_element()
    }

    /// "Turn on <plugin>?": what the first plugin waiting for review will be able to do, over everything else.
    pub(super) fn plugin_review_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.manage.reviews.first()?;
        let more = self.manage.reviews.len() > 1;
        // Turn on answers only once the review has been up its moment.
        let busy = self.manage.busy.contains(&shown.manifest.id) || !self.manage.review_armed;
        let focus = &self.review_focus;
        let (backdrop, panel) = ui::dialog(
            "plugin-review",
            format!("Turn on {}?", shown.manifest.name),
            focus,
            448.0,
            520.0,
        );
        let changed = !shown.approved && shown.enabled;
        Some(
            backdrop
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.skip_review(cx)))
                .child(ui::dialog_panel(
                    panel.h_auto()
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                            if event.keystroke.key == "escape" {
                                this.skip_review(cx);
                                cx.stop_propagation();
                            }
                        }))
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap(px(16.0))
                                .p(px(24.0))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(4.0))
                                        .child(
                                            div()
                                                .flex()
                                                .items_baseline()
                                                .gap(px(12.0))
                                                .child(div().text_size(px(16.0)).font_weight(FontWeight::SEMIBOLD).child(format!("Turn on {}?", shown.manifest.name)))
                                                .when(more, |title| title.child(muted(format!("1 of {}", self.manage.reviews.len())))),
                                        )
                                        .child(muted(if changed {
                                            "This version asks for different access than you approved before."
                                        } else {
                                            "Here's what it will be able to do."
                                        })),
                                )
                                .child(
                                    div().id("plugin-review-access").max_h(px(320.0)).overflow_y_scroll().child(access_list(&shown.manifest)),
                                )
                                .child(muted("Commands run as you, so only turn on plugins you trust."))
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .gap(px(8.0))
                                        .child(
                                            ui::button("plugin-review-cancel", if more { "Not now" } else { "Cancel" }, Variant::Ghost, false, if more { "Not now" } else { "Cancel" })
                                                .on_click(cx.listener(|this, _, _, cx| this.skip_review(cx))),
                                        )
                                        .child(
                                            ui::button("plugin-review-approve", "Turn on", Variant::Default, busy, "Turn on")
                                                .when(!busy, |button| button.on_click(cx.listener(|this, _, window, cx| this.approve_review(window, cx)))),
                                        ),
                                ),
                        ),))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_review_lists_what_the_manifest_asks_for_in_the_desktops_words() {
        let manifest: Manifest = serde_json::from_value(serde_json::json!({
            "id": "demo", "name": "Demo", "version": "1.0.0", "api": 1, "main": "main.js",
            "permissions": { "exec": ["gh pr view"], "notify": true, "panes": [] },
            "contributes": { "ui": ["badges"], "panels": [{ "id": "p", "title": "P", "icon": "clock", "url": "https://example.com/a?b" }] },
        }))
        .unwrap();
        assert_eq!(
            access_groups(&manifest),
            [
                ("Show these in Wings", vec!["Pane badges".to_owned()]),
                (
                    "Open these sites in a panel from the title bar",
                    vec!["example.com".to_owned()]
                ),
                (
                    "Run these commands as you, with your own sign-ins",
                    vec!["gh pr view".to_owned()]
                ),
                (
                    "Open terminals in your projects and move your typing to any pane",
                    vec![]
                ),
                ("Show desktop notifications, up to 20 a minute", vec![]),
            ]
        );
    }
}
