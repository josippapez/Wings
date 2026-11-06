//! What a plugin may show on a pane, checked as the desktop checks it: a badge in the pane's header
//! (`cleanBadge` in `apps/desktop/src/lib/plugins.ts`) and a status strip under its terminal (`cleanPaneStatus` in
//! `apps/desktop/src/lib/pane-status.ts`). A plugin's data is untrusted: a badge keeps only what it knows, cut to
//! length, and a status that breaks a limit is refused with the reason, so its author sees it.
//!
//! Lengths are JS string units (UTF-16), as the desktop counts them.

use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Neutral,
    Info,
    Success,
    Warning,
    Danger,
    Merged,
}

const TONES: [(&str, Tone); 6] = [
    ("neutral", Tone::Neutral),
    ("info", Tone::Info),
    ("success", Tone::Success),
    ("warning", Tone::Warning),
    ("danger", Tone::Danger),
    ("merged", Tone::Merged),
];

impl Tone {
    fn parse(value: &Value) -> Option<Self> {
        let name = value.as_str()?;
        TONES
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, tone)| *tone)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeIcon {
    PrOpen,
    PrMerged,
    PrClosed,
    PrDraft,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountIcon {
    Changed,
    Push,
    Pull,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub label: String,
    pub value: String,
    pub tone: Option<Tone>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadgeAction {
    pub id: String,
    pub label: String,
    pub primary: bool,
}

/// A plugin's badge in a pane header: a small pill that opens a card with details and actions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Badge {
    pub label: String,
    pub tone: Tone,
    /// A spinner in the pill while the plugin refreshes.
    pub loading: bool,
    pub icon: Option<BadgeIcon>,
    /// Small icon-and-number chips after the label, like commits to push.
    pub counts: Vec<(CountIcon, u32)>,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub rows: Vec<Row>,
    pub actions: Vec<BadgeAction>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StatusItem {
    pub label: Option<String>,
    pub value: String,
    /// 0 to 100: a small meter between the label and the value.
    pub percent: Option<f64>,
    pub tone: Option<Tone>,
}

/// What a plugin shows under a pane's terminal.
#[derive(Clone, Debug, PartialEq)]
pub struct PaneStatus {
    pub items: Vec<StatusItem>,
    /// The lines of the card shown when you point at the strip.
    pub rows: Vec<Row>,
    /// The id of one of the plugin's sidebars or views, which a click opens.
    pub open: Option<String>,
}

pub const STATUS_MAX_ITEMS: usize = 4;
pub const STATUS_MAX_ROWS: usize = 12;
pub const STATUS_LABEL_MAX: usize = 16;
pub const STATUS_VALUE_MAX: usize = 32;
const ROW_LABEL_MAX: usize = 40;
const ROW_VALUE_MAX: usize = 200;

/// JS's `string.length`.
pub(in crate::page::plugin) fn units(text: &str) -> usize {
    text.encode_utf16().count()
}

/// JS's `text.slice(0, max)`, without splitting a character in two.
pub(in crate::page::plugin) fn slice(text: &str, max: usize) -> String {
    let mut used = 0;
    text.chars()
        .take_while(|c| {
            used += c.len_utf16();
            used <= max
        })
        .collect()
}

/// `str(v, max)`: the text of a string cut to `max`, or nothing for anything else.
fn cut(value: Option<&Value>, max: usize) -> Option<String> {
    value?.as_str().map(|text| slice(text, max))
}

/// An array's items, which is none for anything else, and at most `max` of them.
fn items(value: Option<&Value>, max: usize) -> impl Iterator<Item = &Value> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(max)
}

/// `Number.isInteger`, for a JSON number.
fn integer(value: Option<&Value>) -> Option<f64> {
    value?
        .as_f64()
        .filter(|number| number.is_finite() && number.fract() == 0.0)
}

/// A badge from what the plugin sent, or `None` when there is no label to show, which clears the pane's badge.
pub fn clean_badge(raw: &Value) -> Option<Badge> {
    let badge = raw.as_object()?;
    let label = cut(badge.get("label"), 40).filter(|label| !label.is_empty())?;
    let icon = match badge.get("icon").and_then(Value::as_str) {
        Some("pr-open") => Some(BadgeIcon::PrOpen),
        Some("pr-merged") => Some(BadgeIcon::PrMerged),
        Some("pr-closed") => Some(BadgeIcon::PrClosed),
        Some("pr-draft") => Some(BadgeIcon::PrDraft),
        _ => None,
    };
    let counts = items(badge.get("counts"), 3)
        .filter_map(|count| {
            let icon = match count.get("icon").and_then(Value::as_str)? {
                "changed" => CountIcon::Changed,
                "push" => CountIcon::Push,
                "pull" => CountIcon::Pull,
                _ => return None,
            };
            let value = integer(count.get("value")).filter(|value| *value > 0.0)?;
            Some((icon, value.min(99_999.0) as u32))
        })
        .collect();
    let rows = items(badge.get("rows"), 12)
        .filter_map(|row| {
            let label = cut(row.get("label"), 40).filter(|text| !text.is_empty())?;
            let value = cut(row.get("value"), 200).filter(|text| !text.is_empty())?;
            // No tone is no tone, but a tone that isn't one of the six is neutral, as for the badge.
            let tone = row
                .get("tone")
                .filter(|tone| is_truthy(tone))
                .map(|tone| Tone::parse(tone).unwrap_or_default());
            Some(Row { label, value, tone })
        })
        .collect();
    let actions = items(badge.get("actions"), 4)
        .filter_map(|action| {
            Some(BadgeAction {
                id: cut(action.get("id"), 40).filter(|text| !text.is_empty())?,
                label: cut(action.get("label"), 40).filter(|text| !text.is_empty())?,
                primary: action.get("primary") == Some(&Value::Bool(true)),
            })
        })
        .collect();
    Some(Badge {
        label,
        tone: badge.get("tone").and_then(Tone::parse).unwrap_or_default(),
        loading: badge.get("loading") == Some(&Value::Bool(true)),
        icon,
        counts,
        title: cut(badge.get("title"), 200),
        subtitle: cut(badge.get("subtitle"), 200),
        rows,
        actions,
    })
}

/// JS truthiness of a JSON value.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(bool) => *bool,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn text(
    value: Option<&Value>,
    what: &str,
    max: usize,
    required: bool,
) -> Result<Option<String>, String> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return if required {
            Err(format!("setPaneStatus: {what} is required"))
        } else {
            Ok(None)
        };
    };
    let Some(text) = value.as_str().filter(|text| !(required && text.is_empty())) else {
        let non_empty = if required { "a non-empty " } else { "" };
        return Err(format!("setPaneStatus: {what} must be {non_empty}string"));
    };
    if units(text) > max {
        return Err(format!("setPaneStatus: {what} is over {max} characters"));
    }
    Ok(Some(text.to_owned()))
}

fn tone_of(value: Option<&Value>, what: &str) -> Result<Option<Tone>, String> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    Tone::parse(value).map(Some).ok_or_else(|| {
        let known: Vec<_> = TONES.iter().map(|(name, _)| *name).collect();
        format!("setPaneStatus: {what} must be one of {}", known.join(", "))
    })
}

fn object<'a>(value: &'a Value, what: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("setPaneStatus: {what} must be an object"))
}

/// A status from what the plugin sent, or the reason it was refused. `pages` are the plugin's sidebar and view ids.
pub fn clean_pane_status(raw: &Value, pages: &[String]) -> Result<PaneStatus, String> {
    let status = raw
        .as_object()
        .ok_or("setPaneStatus: status must be an object or null")?;
    let raw_items = match status.get("items").and_then(Value::as_array) {
        Some(items) if !items.is_empty() => items,
        _ => return Err("setPaneStatus: items must be a non-empty array".into()),
    };
    if raw_items.len() > STATUS_MAX_ITEMS {
        return Err(format!("setPaneStatus: at most {STATUS_MAX_ITEMS} items"));
    }
    let mut items = Vec::new();
    for (i, item) in raw_items.iter().enumerate() {
        let item_object = object(item, &format!("items[{i}]"))?;
        let label = text(
            item_object.get("label"),
            &format!("items[{i}].label"),
            STATUS_LABEL_MAX,
            false,
        )?;
        let value = text(
            item_object.get("value"),
            &format!("items[{i}].value"),
            STATUS_VALUE_MAX,
            true,
        )?
        .expect("a required text is there");
        let percent = match item_object.get("percent").filter(|value| !value.is_null()) {
            None => None,
            Some(percent) => Some(
                percent
                    .as_f64()
                    .filter(|percent| (0.0..=100.0).contains(percent))
                    .ok_or_else(|| {
                        format!("setPaneStatus: items[{i}].percent must be a number from 0 to 100")
                    })?,
            ),
        };
        items.push(StatusItem {
            label: label.filter(|label| !label.is_empty()),
            value,
            percent,
            tone: tone_of(item_object.get("tone"), &format!("items[{i}].tone"))?,
        });
    }
    let mut rows = Vec::new();
    if let Some(raw_rows) = status.get("rows").filter(|value| !value.is_null()) {
        let Some(raw_rows) = raw_rows.as_array() else {
            return Err("setPaneStatus: rows must be an array".into());
        };
        if raw_rows.len() > STATUS_MAX_ROWS {
            return Err(format!("setPaneStatus: at most {STATUS_MAX_ROWS} rows"));
        }
        for (i, row) in raw_rows.iter().enumerate() {
            let row_object = object(row, &format!("rows[{i}]"))?;
            rows.push(Row {
                label: text(
                    row_object.get("label"),
                    &format!("rows[{i}].label"),
                    ROW_LABEL_MAX,
                    true,
                )?
                .expect("a required text is there"),
                value: text(
                    row_object.get("value"),
                    &format!("rows[{i}].value"),
                    ROW_VALUE_MAX,
                    true,
                )?
                .expect("a required text is there"),
                tone: tone_of(row_object.get("tone"), &format!("rows[{i}].tone"))?,
            });
        }
    }
    let open = match status.get("open").filter(|value| !value.is_null()) {
        None => None,
        Some(open) => match open
            .as_str()
            .filter(|open| pages.iter().any(|page| page == open))
        {
            Some(open) => Some(open.to_owned()),
            None => {
                let known = if pages.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", pages.join(", "))
                };
                return Err(format!(
                    "setPaneStatus: open must be the id of one of the plugin's sidebars or views{known}"
                ));
            }
        },
    };
    Ok(PaneStatus { items, rows, open })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_badge_keeps_what_it_knows_cut_to_length() {
        let long = "é".repeat(300);
        let badge = clean_badge(&json!({
            "label": long, "tone": "success", "loading": true, "icon": "pr-merged",
            "counts": [
                { "icon": "push", "value": 3 }, { "icon": "pull", "value": 0 }, { "icon": "other", "value": 2 },
                { "icon": "changed", "value": 1e9 }, { "icon": "changed", "value": 2.5 }, { "icon": "pull", "value": 4 },
            ],
            "title": "t".repeat(300), "subtitle": "s",
            "rows": [{ "label": "Checks", "value": "passing", "tone": "danger" }, { "label": "", "value": "x" }, { "label": "a", "value": "b", "tone": "weird" }],
            "actions": [{ "id": "open", "label": "Open", "primary": true }, { "id": "", "label": "No id" }, { "id": "x" }],
        }))
        .unwrap();
        assert_eq!(badge.label.chars().count(), 40);
        assert_eq!(
            (badge.tone, badge.loading, badge.icon),
            (Tone::Success, true, Some(BadgeIcon::PrMerged))
        );
        // Three counts at most are looked at; of those a zero and an unknown icon are dropped.
        assert_eq!(badge.counts, [(CountIcon::Push, 3)]);
        assert_eq!(badge.title.unwrap().len(), 200);
        assert_eq!(
            badge.rows,
            [
                Row {
                    label: "Checks".into(),
                    value: "passing".into(),
                    tone: Some(Tone::Danger)
                },
                Row {
                    label: "a".into(),
                    value: "b".into(),
                    tone: Some(Tone::Neutral)
                },
            ]
        );
        assert_eq!(
            badge.actions,
            [BadgeAction {
                id: "open".into(),
                label: "Open".into(),
                primary: true
            }]
        );
    }

    #[test]
    fn a_badge_with_no_label_or_no_object_is_none() {
        for raw in [
            json!(null),
            json!("x"),
            json!({}),
            json!({ "label": "" }),
            json!({ "label": 3 }),
        ] {
            assert_eq!(clean_badge(&raw), None, "{raw}");
        }
        let plain = clean_badge(&json!({ "label": "localhost:5173", "tone": "nope" })).unwrap();
        assert_eq!(plain.tone, Tone::Neutral);
        assert!(plain.counts.is_empty() && plain.rows.is_empty() && plain.actions.is_empty());
    }

    fn status(raw: Value) -> Result<PaneStatus, String> {
        clean_pane_status(&raw, &["hud".to_owned(), "board".to_owned()])
    }

    #[test]
    fn a_status_keeps_its_items_rows_and_page() {
        let status = status(json!({
            "items": [{ "label": "5h", "value": "23%", "percent": 23, "tone": "warning" }, { "value": "ok" }],
            "rows": [{ "label": "Resets", "value": "in 2h", "tone": "info" }],
            "open": "hud",
        }))
        .unwrap();
        assert_eq!(status.items.len(), 2);
        assert_eq!(status.items[0].percent, Some(23.0));
        assert_eq!(status.items[0].tone, Some(Tone::Warning));
        assert_eq!(status.items[1].label, None);
        assert_eq!(status.rows[0].tone, Some(Tone::Info));
        assert_eq!(status.open.as_deref(), Some("hud"));
    }

    #[test]
    fn a_status_that_breaks_a_limit_is_refused_with_the_reason() {
        let item = |extra: Value| {
            let mut item = json!({ "value": "v" });
            item.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            json!({ "items": [item] })
        };
        let refused = |raw: Value| status(raw).unwrap_err();
        assert_eq!(
            refused(json!("x")),
            "setPaneStatus: status must be an object or null"
        );
        assert_eq!(
            refused(json!({ "items": [] })),
            "setPaneStatus: items must be a non-empty array"
        );
        assert_eq!(
            refused(json!({ "items": vec![json!({ "value": "a" }); 5] })),
            "setPaneStatus: at most 4 items"
        );
        assert_eq!(
            refused(json!({ "items": [1] })),
            "setPaneStatus: items[0] must be an object"
        );
        assert_eq!(
            refused(json!({ "items": [{}] })),
            "setPaneStatus: items[0].value is required"
        );
        assert_eq!(
            refused(json!({ "items": [{ "value": "" }] })),
            "setPaneStatus: items[0].value must be a non-empty string"
        );
        assert_eq!(
            refused(item(json!({ "value": "x".repeat(33) }))),
            "setPaneStatus: items[0].value is over 32 characters"
        );
        assert_eq!(
            refused(item(json!({ "label": "x".repeat(17) }))),
            "setPaneStatus: items[0].label is over 16 characters"
        );
        assert_eq!(
            refused(item(json!({ "label": 3 }))),
            "setPaneStatus: items[0].label must be string"
        );
        assert_eq!(
            refused(item(json!({ "percent": 101 }))),
            "setPaneStatus: items[0].percent must be a number from 0 to 100"
        );
        assert_eq!(
            refused(item(json!({ "percent": "5" }))),
            "setPaneStatus: items[0].percent must be a number from 0 to 100"
        );
        assert_eq!(
            refused(item(json!({ "tone": "loud" }))),
            "setPaneStatus: items[0].tone must be one of neutral, info, success, warning, danger, merged"
        );
        let rows = |rows: Value| json!({ "items": [{ "value": "v" }], "rows": rows });
        assert_eq!(
            refused(rows(json!("x"))),
            "setPaneStatus: rows must be an array"
        );
        assert_eq!(
            refused(rows(json!(vec![json!({ "label": "a", "value": "b" }); 13]))),
            "setPaneStatus: at most 12 rows"
        );
        assert_eq!(
            refused(rows(json!([{ "label": "a" }]))),
            "setPaneStatus: rows[0].value is required"
        );
        let open = |open: Value| json!({ "items": [{ "value": "v" }], "open": open });
        assert_eq!(
            refused(open(json!("elsewhere"))),
            "setPaneStatus: open must be the id of one of the plugin's sidebars or views (hud, board)"
        );
        assert_eq!(
            clean_pane_status(&open(json!("x")), &[]).unwrap_err(),
            "setPaneStatus: open must be the id of one of the plugin's sidebars or views"
        );
    }
}
