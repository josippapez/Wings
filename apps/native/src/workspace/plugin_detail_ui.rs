//! A plugin's own page in Settings, Plugins: opened from its row, in place of the list. Back to Plugins, its icon, name
//! and byline, the same action slot, switch and more menu as its row, then what it adds to Wings, what it can access,
//! its settings page and its README, with an info column beside them when there is room.

use gpui::{
    AnyElement, ClipboardItem, Context, ElementId, FontWeight, div, prelude::*, px, rgb, rgba,
};
use wings_plugins::store::{PluginView, Source};

use crate::{
    icons::{Icon, icon},
    theme,
    ui::{self, ButtonSize, Variant},
    workspace::NotifyWorkspace,
};

use super::{
    Workspace,
    plugin_manage_ui::{Open, access_groups, access_list, adds_to_wings, access_summary, muted, plugin_icon, source_label, status_word},
};

/// The info column is this wide, and shows when the page has this much room: its column of sections stays readable.
const INFO_W: f32 = 220.0;
const INFO_FROM: f32 = 660.0;

fn heading(text: &str) -> gpui::Div {
    div().text_size(px(13.0)).font_weight(FontWeight::SEMIBOLD).child(text.to_owned())
}

/// One fact of the info column: its name small and muted over its value.
fn fact(label: &str, value: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(muted(label.to_owned()).text_size(px(11.0)))
        .child(div().text_size(px(12.0)).child(value))
}

impl Workspace {
    /// The plugin's page.
    pub(super) fn plugin_detail(&self, view: PluginView, content_w: f32, cx: &mut Context<Self>) -> AnyElement {
        let manage = &self.manage;
        let id = view.manifest.id.clone();
        let on = view.enabled && view.approved;
        let runs = self.plugin_runs(&id);
        let busy = manage.busy.contains(&id);
        let update = manage.updates.get(&id).cloned();
        let (word, tone) = status_word(view.approved, view.enabled, runs, update.as_deref());
        let javascript = self.built_for_javascript(&view);
        let wide = content_w >= INFO_FROM;

        let back = ui::button(
            "plugin-back",
            "Back to Plugins",
            Variant::Ghost,
            false,
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(icon(Icon::ArrowLeft, 14.0, theme::muted_foreground()))
                .child("Plugins")
                .children(ui::spot(|| "plugin-back".into())),
        )
        .h(px(28.0))
        .px(px(8.0))
        .text_size(px(13.0))
        .on_click(cx.listener(|this, _, _, cx| {
            this.manage.selected = None;
            this.manage.removing = None;
            this.manage.menu = None;
            cx.notify_workspace();
        }));

        let head = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(12.0))
            .child(plugin_icon(on, view.icon_svg.as_deref(), 48.0))
            .child(
                div()
                    .flex_1()
                    .min_w(px(160.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(16.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(view.manifest.name.clone()),
                            )
                            .child(ui::badge(Variant::Secondary, Some(tone), true, word)),
                    )
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(self.action_slot("plugin-detail", 0, &view, cx))
                    .child(self.plugin_switch("plugin-detail", 0, &view, cx))
                    .child(self.more_button("plugin-detail", 0, &id, cx)),
            );

        let remove = (manage.removing.as_deref() == Some(id.as_str()) && !view.dev).then(|| {
            let gone = id.clone();
            ui::card([div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.0))
                .px(px(16.0))
                .py(px(12.0))
                .child(div().flex_1().min_w(px(180.0)).child(format!("Remove {} from Wings?", view.manifest.name)))
                .child(
                    ui::button("plugin-remove-confirm", "Remove plugin", Variant::Destructive, busy, "Remove plugin")
                        .when(!busy, |button| {
                            button.on_click(cx.listener(move |this, _, window, cx| {
                                this.remove_plugin_now(&gone, window, cx)
                            }))
                        }),
                )
                .child(
                    ui::button("plugin-keep", "Keep", Variant::Ghost, false, "Keep").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.manage.removing = None;
                            cx.notify_workspace();
                        },
                    )),
                )
                .into_any_element()])
        });
        let notice = |text: &'static str| ui::card([muted(text).px(px(16.0)).py(px(12.0)).into_any_element()]);
        let error = manage.row_error.get(&id).cloned();

        let adds = adds_to_wings(&view.manifest);
        let can_open = on && runs;
        let adds_rows: Vec<AnyElement> = adds
            .into_iter()
            .enumerate()
            .map(|(index, add)| {
                let plugin = id.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .px(px(16.0))
                    .py(px(10.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().text_size(px(13.0)).font_weight(FontWeight::MEDIUM).child(add.name))
                            .child(muted(add.kind)),
                    )
                    .children(add.open.filter(|_| can_open).map(|open| {
                        small_open(index).on_click(cx.listener(move |this, _, window, cx| match &open {
                            Open::Page(page) => {
                                this.close_settings(window, cx);
                                this.open_plugin_page(&plugin, page, window, cx);
                            }
                            Open::Settings => this.open_plugin_settings(&plugin, window, cx),
                        }))
                    }))
                    .into_any_element()
            })
            .collect();

        let main = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .child(muted(match view.manifest.description.trim() {
                "" => "This plugin has no description.".to_owned(),
                description => description.to_owned(),
            }).text_size(px(13.0)))
            .when(!adds_rows.is_empty(), |page| {
                page.child(div().flex().flex_col().gap(px(8.0)).child(heading("Adds to Wings")).child(ui::card(adds_rows)))
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(heading("Can access"))
                    .child(muted(access_summary(&view.manifest)))
                    .when(!access_groups(&view.manifest).is_empty(), |section| {
                        section.child(access_list(&view.manifest))
                    }),
            )
            .when_some(self.plugin_readme(&view), |page, blocks| {
                page.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .pt(px(16.0))
                        .border_t_1()
                        .border_color(rgba(theme::border()))
                        .child(heading("README"))
                        .child(crate::markdown::render("plugin-readme", &blocks))
                        .children(ui::spot(|| "plugin-readme".into())),
                )
            });

        div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .child(div().flex().child(back))
            .child(head)
            .children(remove)
            .when_some(error, |page, error| {
                page.child(div().text_size(px(12.0)).text_color(rgb(theme::danger())).child(error))
            })
            .when(!view.approved, |page| {
                page.child(notice(if view.enabled {
                    "This version asks for different access than you approved before. Review it to turn the plugin on again."
                } else {
                    "It is off until you review what it can access and turn it on."
                }))
            })
            .when(javascript, |page| page.child(notice(super::plugin_manage_ui::BUILT_FOR_JAVASCRIPT)))
            .child(
                div()
                    .flex()
                    .gap(px(24.0))
                    .items_start()
                    .child(main)
                    .when(wide, |row| row.child(self.info_column(&view, cx))),
            )
            .into_any_element()
    }

    /// Version, source, the Wings it needs, its id and its repository, small, beside the page.
    fn info_column(&self, view: &PluginView, cx: &mut Context<Self>) -> gpui::Div {
        let id = view.manifest.id.clone();
        let github = match (&view.source, view.dev) {
            (Some(Source::Github { repo, .. }), false) => Some(repo.clone()),
            _ => None,
        };
        let requires = match &view.manifest.min_wings_version {
            Some(version) => format!("Wings {version} or newer"),
            None => "Any version".to_owned(),
        };
        div()
            .w(px(INFO_W))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(fact("Version", view.manifest.version.clone()))
            .child(fact("Source", source_label(view)))
            .child(fact("Requires", requires))
            .child(fact(
                "Plugin ID",
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(div().min_w_0().truncate().font_family("Menlo").child(id.clone()))
                    .child(
                        ui::icon_button("plugin-copy-id", Icon::Copy, "Copy ID", ButtonSize::Xs).on_click(
                            cx.listener(move |_, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(id.clone()))
                            }),
                        ),
                    ),
            ))
            .children(github.map(|repo| {
                let url = format!("https://github.com/{repo}");
                fact(
                    "Repository",
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(div().min_w_0().truncate().child(repo))
                        .child(
                            ui::icon_button("plugin-open-repo", Icon::ExternalLink, "Open on GitHub", ButtonSize::Xs)
                                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url))),
                        ),
                )
            }))
    }
}

/// An Open button of the "Adds to Wings" list.
fn small_open(index: usize) -> gpui::Stateful<gpui::Div> {
    ui::button(ElementId::NamedInteger("plugin-open".into(), index as u64), "Open", Variant::Outline, false, "Open")
        .h(px(28.0))
        .px(px(10.0))
        .text_size(px(13.0))
}
