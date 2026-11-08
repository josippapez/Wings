//! Persistent, actionable notices for background plugin checks.

use gpui::{AnyElement, Context, FontWeight, Role, div, prelude::*, px, rgb, rgba};

use crate::{
    theme,
    ui::{self, Variant},
};

use super::{HAIRLINE, Workspace, plugin_manage::Review, settings::Section};

impl Workspace {
    pub(super) fn plugin_update_notices(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.manage.announcements.is_empty() {
            return None;
        }
        Some(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .children(self.manage.announcements.iter().map(|notice| {
                    let index = notice.review.slot();
                    let review = notice.review.clone();
                    div()
                        .id(("plugin-update-notice", index))
                        .role(Role::Alert)
                        .aria_label(notice.title.clone())
                        .p(px(12.0))
                        .rounded(px(12.0))
                        .border_1()
                        .border_color(rgba(HAIRLINE))
                        .bg(rgba(theme::POPOVER))
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(notice.title.clone()),
                        )
                        .when(!notice.body.is_empty(), |card| {
                            card.child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(rgb(theme::MUTED_FOREGROUND))
                                    .child(notice.body.clone()),
                            )
                        })
                        .child(
                            ui::dialog_footer()
                                .child(
                                    ui::button(
                                        ("plugin-update-dismiss", index),
                                        "Dismiss",
                                        Variant::Ghost,
                                        false,
                                        "Dismiss",
                                    )
                                    .children(ui::spot(move || {
                                        format!("plugin-update-dismiss-{index}")
                                    }))
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.manage
                                                .announcements
                                                .retain(|notice| notice.review.slot() != index);
                                            cx.notify();
                                        },
                                    )),
                                )
                                .child(
                                    ui::button(
                                        ("plugin-update-review", index),
                                        "Review",
                                        Variant::Secondary,
                                        false,
                                        "Review",
                                    )
                                    .children(ui::spot(move || {
                                        format!("plugin-update-review-{index}")
                                    }))
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            this.review_plugin_announcement(
                                                review.clone(),
                                                window,
                                                cx,
                                            );
                                        },
                                    )),
                                ),
                        )
                }))
                .into_any_element(),
        )
    }

    pub(super) fn review_plugin_announcement(
        &mut self,
        review: Review,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings(window, cx);
        if let Some(dialog) = &mut self.settings {
            dialog.section = Section::Plugins;
        }
        self.manage.browsing_available = review == Review::Available;
        self.manage.selected = match &review {
            Review::Installed(id) => Some(id.clone()),
            Review::Available => None,
        };
        self.manage
            .announcements
            .retain(|notice| notice.review.slot() != review.slot());
        cx.notify();
    }
}

#[cfg(feature = "test-support")]
impl Workspace {
    /// Made-up notices and catalogue entries, without network access or installing anything.
    pub fn sample_plugin_updates(
        &mut self,
        state: &str,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        use super::plugin_manage::Announcement;
        self.manage.announcements.clear();
        self.plugin_ui.notices.clear();
        self.manage.available = vec![wings_plugins::store::Available {
            id: "catalog-demo".into(),
            name: "Project notes".into(),
            description: "Keep project notes beside your terminals.".into(),
            version: "1.0.0".into(),
            link: "https://github.com/example/notes".into(),
        }];
        if state == "available" {
            self.review_plugin_announcement(Review::Available, window, cx);
            return;
        }
        self.settings = None;
        self.manage.announcements.push(Announcement {
            title: if state == "single" {
                "Demo 1.2.0 is available"
            } else {
                "Plugin updates available"
            }
            .into(),
            body: if state == "single" {
                ""
            } else {
                "Demo 1.2.0, Change journal 2.1.0"
            }
            .into(),
            review: Review::Installed("demo".into()),
        });
        if state != "single" {
            self.manage.announcements.push(Announcement {
                title: "New plugin: Project notes".into(),
                body: "Keep project notes beside your terminals.".into(),
                review: Review::Available,
            });
            self.show_notice("A plugin notification".into(), "This longer message shares the same stack and wraps without covering another notice.".into(), cx);
        }
        cx.notify();
    }
}
