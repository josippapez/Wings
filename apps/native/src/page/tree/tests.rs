use super::*;
use serde_json::json;

fn tree(children: Value) -> Result<Tree, TreeError> {
    Tree::parse(&json!({"id":0,"type":"#root","props":{},"children":children}).to_string())
}

fn button(props: Value) -> Value {
    json!({"id":2,"type":"Button","props":props,"children":[{"id":3,"type":"#text","text":"Go"}]})
}

#[test]
fn a_page_paints_in_the_colours_of_its_palette_and_the_apps_for_the_rest() {
    let painted = |palette: Value| {
        Tree::parse(&json!({"id":0,"type":"#root","props":{"palette":palette},"children":[]}).to_string())
            .unwrap()
            .palette
    };
    let palette = painted(json!({
        "page": "#1f1a15", "text": "#F0E7D7", "popover": "#3a3229",
        // Not colours: the app's stay.
        "muted": "red", "info": "#12345", "danger": 7,
        "ground": {"from": "#1f1a15", "to": "#312a22", "angle": -90},
    }))
    .unwrap();
    assert_eq!((palette.surface, palette.foreground), (0x1f1a15, 0xf0e7d7));
    assert_eq!(palette.popover, 0x3a3229f2, "see-through as the app's popover");
    let app = crate::theme::Palette::APP;
    assert_eq!((palette.muted_foreground, palette.info, palette.danger), (app.muted_foreground, app.info, app.danger));
    assert_eq!(palette.ground, Some((0x1f1a15, 0x312a22, 270.0)));
    // Nothing it can paint is no palette at all.
    assert_eq!(painted(json!({"page": "url(x)"})), None);
    assert_eq!(tree(json!([])).unwrap().palette, None);
}

/// A page of one `Page` with a toolbar fills the frame without asking, so its toolbar is not inside the shell's padding.
#[test]
fn a_page_with_a_toolbar_fills_the_frame_without_asking() {
    let toolbar = json!({"id":4,"type":"Slot","props":{"name":"toolbar"},"children":[
        {"id":5,"type":"Toolbar","props":{},"children":[]}]});
    let page = |slots: Vec<Value>| json!({"id":1,"type":"Page","props":{},"children":slots});
    assert!(tree(json!([page(vec![toolbar])])).unwrap().fill);
    assert!(!tree(json!([page(vec![])])).unwrap().fill, "no toolbar: the shell pads it");
    assert!(!tree(json!([button(json!({}))])).unwrap().fill, "not a page");
}

#[test]
fn parses_a_page_into_its_parts() {
    let parsed = tree(json!([{"id":1,"type":"Page","props":{"flush":true},"children":[
        {"id":4,"type":"Slot","props":{"name":"toolbar"},"children":[
            {"id":5,"type":"Toolbar","props":{},"children":[
                {"id":6,"type":"Slot","props":{"name":"title"},"children":[{"id":7,"type":"#text","text":"Pilot"}]}]}]},
        {"id":8,"type":"Slot","props":{"name":"body"},"children":[
            button(json!({"variant":"outline","onClick":"2:onClick"}))]}]}]))
    .unwrap();
    let Node::Page(page) = &parsed.children[0] else {
        panic!("a page")
    };
    assert!(page.flush);
    let Node::Toolbar(toolbar) = &page.toolbar[0] else {
        panic!("a toolbar")
    };
    assert_eq!(plain_text(&toolbar.title), "Pilot");
    let Node::Button(button) = &page.body[0] else {
        panic!("a button")
    };
    assert_eq!(button.variant, Variant::Outline);
    assert_eq!(button.on_click.as_deref(), Some("2:onClick"));
    assert_eq!(parsed.text(), "Pilot Go");
}

#[test]
fn a_button_and_a_text_keep_their_title_and_none_when_it_is_absent_or_empty() {
    let parsed = tree(json!([
        button(json!({"title":"Opens a terminal"})),
        button(json!({"title":""})),
        button(json!({})),
        {"id":50,"type":"Text","props":{"title":"/work"},"children":[]},
        {"id":51,"type":"Text","props":{},"children":[]},
    ]))
    .unwrap();
    let titles: Vec<_> = parsed
        .children
        .iter()
        .map(|node| match node {
            Node::Button(button) => button.title.as_deref(),
            Node::Text(text) => text.title.as_deref(),
            _ => panic!("a button or a text"),
        })
        .collect();
    assert_eq!(
        titles,
        [Some("Opens a terminal"), None, None, Some("/work"), None]
    );
}

#[test]
fn a_list_button_keeps_its_title() {
    let parsed = tree(json!([
        {"id":1,"type":"ListButton","props":{"title":"Open Changes"},"children":[]},
        {"id":2,"type":"ListButton","props":{},"children":[]},
    ]))
    .unwrap();
    let titles: Vec<_> = parsed
        .children
        .iter()
        .map(|node| match node {
            Node::ListButton(item) => item.button.title.as_deref(),
            _ => panic!("a list button"),
        })
        .collect();
    assert_eq!(titles, [Some("Open Changes"), None]);
}

#[test]
fn a_button_keeps_its_size_and_a_chip_its_pressed_state() {
    let parsed = tree(json!([
        button(json!({"size":"sm"})),
        button(json!({"chip":true,"pressed":true})),
        button(json!({"size":"huge"})),
    ]))
    .unwrap();
    let buttons: Vec<_> = parsed
        .children
        .iter()
        .map(|node| match node {
            Node::Button(button) => (button.size, button.chip, button.pressed),
            _ => panic!("a button"),
        })
        .collect();
    assert_eq!(
        buttons,
        [
            (ButtonSize::Sm, false, false),
            (ButtonSize::Default, true, true),
            (ButtonSize::Default, false, false),
        ]
    );
}

#[test]
fn a_button_is_round_only_when_it_says_so() {
    let parsed = tree(json!([
        button(json!({"size":"icon-sm","round":true})),
        button(json!({"round":"yes"})),
        button(json!({})),
    ]))
    .unwrap();
    let round: Vec<_> = parsed
        .children
        .iter()
        .map(|node| matches!(node, Node::Button(button) if button.round))
        .collect();
    assert_eq!(round, [true, false, false]);
}

#[test]
fn unknown_components_are_dropped_with_what_is_in_them() {
    let parsed = tree(json!([
        {"id":1,"type":"Hologram","props":{},"children":[{"id":2,"type":"#text","text":"never"}]},
        {"id":3,"type":"Stack","props":{},"children":[
            {"id":4,"type":"script","props":{},"children":[]},
            {"id":5,"type":"#text","text":"kept"}]},
    ]))
    .unwrap();
    assert_eq!(parsed.children.len(), 1);
    assert_eq!(parsed.text(), "kept");
}

#[test]
fn unknown_values_are_the_defaults_and_a_bad_handler_is_none() {
    let parsed = tree(json!([
        button(json!({"variant":"flashy","onClick":"../../etc"})),
        button(json!({"onClick":"2:onClick","disabled":true})),
        {"id":9,"type":"Stack","props":{"gap":"huge"},"children":[]},
    ]))
    .unwrap();
    let Node::Button(first) = &parsed.children[0] else {
        panic!("a button")
    };
    assert_eq!(
        (first.variant, first.on_click.clone()),
        (Variant::Default, None)
    );
    let Node::Button(disabled) = &parsed.children[1] else {
        panic!("a button")
    };
    assert!(disabled.disabled && disabled.on_click.is_none());
    let Node::Stack(stack) = &parsed.children[2] else {
        panic!("a stack")
    };
    assert_eq!(stack.gap, Gap::M);
}

#[test]
fn notices_spinners_checkboxes_and_confirms_have_only_their_supported_parts() {
    let parsed = tree(json!([
        {"id":1,"type":"Notice","props":{"tone":"warn"},"children":[text(2, "Careful")]},
        {"id":3,"type":"Spinner","props":{"size":"large","label":"Refreshing"},"children":[]},
        {"id":4,"type":"Checkbox","props":{"checked":true,"onChange":"4:onChange"},"children":[text(5, "Delete branch")]},
        {"id":6,"type":"Confirm","props":{},"children":[
            slot("title", json!([text(7, "Remove?")])),
            slot("body", json!([text(8, "This cannot be undone.")])),
            slot("actions", json!([button(json!({"onClick":"9:onClick"}))]))]}]))
    .unwrap();
    let Node::Notice(notice) = &parsed.children[0] else {
        panic!("a notice")
    };
    assert_eq!(
        (notice.tone, plain_text(&notice.children)),
        (Some(Tone::Warn), "Careful".to_owned())
    );
    let Node::Spinner(spinner) = &parsed.children[1] else {
        panic!("a spinner")
    };
    assert!(spinner.large && spinner.label == "Refreshing");
    let Node::Checkbox(checkbox) = &parsed.children[2] else {
        panic!("a checkbox")
    };
    assert_eq!(
        (
            checkbox.checked,
            checkbox.on_change.as_deref(),
            plain_text(&checkbox.children)
        ),
        (true, Some("4:onChange"), "Delete branch".to_owned())
    );
    let Node::Confirm(confirm) = &parsed.children[3] else {
        panic!("a confirm")
    };
    assert_eq!(plain_text(&confirm.title), "Remove?");
    assert_eq!(plain_text(&confirm.body), "This cannot be undone.");
    assert_eq!(plain_text(&confirm.actions), "Go");
}

fn slot(name: &str, children: Value) -> Value {
    json!({"id":90,"type":"Slot","props":{"name":name},"children":children})
}

fn text(id: u64, text: &str) -> Value {
    json!({"id":id,"type":"#text","text":text})
}

fn parse_one(node: Value) -> Node {
    tree(json!([node])).unwrap().children.remove(0)
}

#[test]
fn a_chat_column_rises_in_a_scroll_that_follows_its_end_and_its_thinking_shimmers() {
    let Node::Scroll(scroll) = parse_one(json!({"id":1,"type":"Scroll","props":{"follow":"end"},"children":[
        {"id":2,"type":"Stack","props":{"enter":"rise"},"children":[
            {"id":3,"type":"Text","props":{"shimmer":true},"children":[text(4, "Thinking")]}
        ]}
    ]})) else { panic!() };
    assert!(scroll.follow_end && scroll.on_reach_start.is_none());
    let Node::Scroll(older) = parse_one(json!({"id":1,"type":"Scroll","props":{"follow":"end","onReachStart":"1:onReachStart"},"children":[]})) else { panic!() };
    assert_eq!(older.on_reach_start.as_deref(), Some("1:onReachStart"));
    let Node::Stack(column) = &scroll.children[0] else { panic!() };
    assert!(column.enter);
    let Node::Text(thinking) = &column.children[0] else { panic!() };
    assert!(thinking.shimmer);
    assert_eq!(thinking.since, None);
    let Node::Text(running) = parse_one(json!({"id":1,"type":"Text","props":{"shimmer":true,"since":1_763_470_000_000u64},"children":[text(2, "Running pnpm test")]})) else { panic!() };
    assert_eq!(running.since, Some(1_763_470_000_000));
    assert_eq!(thinking.children[0].id(), Some(4));
    let Node::Scroll(plain) = parse_one(json!({"id":1,"type":"Scroll","props":{},"children":[
        {"id":2,"type":"Stack","props":{"enter":"fade"},"children":[]}
    ]})) else { panic!() };
    assert!(!plain.follow_end);
    assert!(matches!(&plain.children[0], Node::Stack(stack) if !stack.enter));
}

#[test]
fn a_scroll_takes_a_place_to_scroll_to_and_children_to_track() {
    let scroll = |props: Value| match parse_one(json!({"id":1,"type":"Scroll","props":props,"children":[]})) {
        Node::Scroll(scroll) => scroll,
        _ => panic!(),
    };
    let plain = scroll(json!({}));
    assert_eq!((plain.scroll_to, plain.track.is_empty(), plain.on_track), (None, true, None));
    let aimed = scroll(json!({"scrollTo":{"index":7,"token":3},"track":[2,5,9,"x",-1],"onTrack":"1:onTrack"}));
    assert_eq!(aimed.scroll_to, Some((7, 3)));
    assert_eq!(aimed.track, [2, 5, 9]);
    assert_eq!(aimed.on_track.as_deref(), Some("1:onTrack"));
    // A place that isn't two whole numbers is no place.
    assert_eq!(scroll(json!({"scrollTo":{"index":-1,"token":3}})).scroll_to, None);
    assert_eq!(scroll(json!({"scrollTo":{"index":1}})).scroll_to, None);
    // A slim pane, like a rail, is as narrow as asked down to 16 points.
    let Node::Split(rail) = parse_one(json!({"id":1,"type":"Split","props":{"defaultSize":26},"children":[]})) else { panic!() };
    assert_eq!(rail.size, 26.0);
}

#[test]
fn a_scroll_takes_a_text_scale_within_reason() {
    let scale = |props: Value| match parse_one(json!({"id":1,"type":"Scroll","props":props,"children":[]})) {
        Node::Scroll(scroll) => scroll.text_scale,
        _ => panic!(),
    };
    assert_eq!(scale(json!({})), None);
    assert_eq!(scale(json!({"textScale":1.25})), Some(1.25));
    assert_eq!(scale(json!({"textScale":40})), Some(2.0));
    assert_eq!(scale(json!({"textScale":0})), Some(0.5));
    assert_eq!(scale(json!({"textScale":"big"})), None);
}

/// A Markdown quote's bar is a prop of the stack, so it is drawn by the box and never copied as a word.
#[test]
fn a_stack_with_quote_set_is_a_quote_and_a_plain_one_is_not() {
    let quote = |props: serde_json::Value| match parse_one(json!({"id":1,"type":"Stack","props":props,"children":[]})) {
        Node::Stack(stack) => stack.quote,
        _ => panic!("a stack"),
    };
    assert!(quote(json!({"quote":true})));
    assert!(!quote(json!({})));
    assert!(!quote(json!({"quote":"yes"})));
}

/// A plugin's settings page grouped its rows in a plain card, with no hairlines between them as Wings' own have.
#[test]
fn a_card_of_rows_says_so_and_a_plain_or_raised_one_does_not() {
    let card = |variant: serde_json::Value| match parse_one(json!({"id":1,"type":"Card","props":{"variant":variant},"children":[]})) {
        Node::Card(card) => (card.rows, card.raised),
        _ => panic!("a card"),
    };
    assert_eq!(card(json!("rows")), (true, false));
    let Node::Card(scrolls) = parse_one(json!({"id":1,"type":"Card","props":{"scroll":true},"children":[]})) else { panic!("a card") };
    assert!(scrolls.scrolls);
    assert_eq!(card(json!("raised")), (false, true));
    assert_eq!(card(serde_json::Value::Null), (false, false));
}

#[test]
fn card_heading_list_and_list_item_hold_their_children() {
    let Node::Card(card) = parse_one(json!({"id":1,"type":"Card","props":{},"children":[
        {"id":2,"type":"Heading","props":{},"children":[text(3, "Usage")]},
        {"id":4,"type":"List","props":{},"children":[
            {"id":5,"type":"ListItem","props":{},"children":[text(6, "one")]},
            // A slot out of place has nothing to belong to.
            slot("note", json!([text(7, "lost")]))]}]}))
    else {
        panic!("a card")
    };
    assert_eq!(plain_text(&card.children), "Usageone");
    let [Node::Heading(_), Node::List(list)] = card.children.as_slice() else {
        panic!("a heading and a list")
    };
    assert!(matches!(list.children.as_slice(), [Node::ListItem(_)]));
}

#[test]
fn a_list_items_actions_slot_is_its_actions_and_not_one_of_its_parts() {
    let Node::ListItem(item) = parse_one(
        json!({"id":1,"type":"ListItem","props":{},"children":[
        text(2, "Code review"),
        slot("actions", json!([{"id":4,"type":"Button","props":{"onClick":"4:click"},"children":[text(5, "Edit")]}]))]}),
    ) else {
        panic!("a list item")
    };
    assert_eq!(plain_text(&item.children), "Code review");
    assert!(matches!(item.actions.as_slice(), [Node::Button(_)]));
}

#[test]
fn a_heading_of_level_3_names_a_section() {
    let section = |props: Value| match parse_one(
        json!({"id":1,"type":"Heading","props":props,"children":[text(2, "Entries")]}),
    ) {
        Node::Heading(heading) => heading.section,
        other => panic!("a heading, not {other:?}"),
    };
    // It was drawn as every other heading, small and muted, where the desktop draws a section's 14px title.
    assert!(section(json!({"level":3})));
    assert!(!section(json!({"level":2})));
    assert!(!section(json!({})));
}

#[test]
fn only_a_text_input_of_type_password_is_a_password() {
    let password = |kind: &str, kind_prop: &str| match parse_one(
        json!({"id":1,"type":kind,"props":{"label":"Token","type":kind_prop},"children":[]}),
    ) {
        Node::Input(input) => input.password,
        other => panic!("{other:?}"),
    };
    assert!(password("TextInput", "password"));
    assert!(!password("TextInput", "time"));
    assert!(!password("TextInput", "text"));
    assert!(!password("Textarea", "password"));
}

#[test]
fn a_textarea_takes_shift_tab_and_history_keys_only_when_the_page_asks() {
    let props = |props: Value| match parse_one(json!({"id":1,"type":"Textarea","props":props,"children":[]})) {
        Node::Input(input) => (input.takes_shift_tab, input.history_keys),
        other => panic!("{other:?}"),
    };
    assert_eq!(props(json!({"takesShiftTab":true,"historyKeys":true})), (true, true));
    assert_eq!(props(json!({"takesShiftTab":true})), (true, false));
    assert_eq!(props(json!({"historyKeys":false})), (false, false));
    assert_eq!(props(json!({})), (false, false));
}

/// A row can center what it holds, as a picture in its dialog asks: "center" was read as the start, so the picture sat
/// at the dialog's left.
#[test]
fn a_row_centers_what_it_holds_when_asked() {
    let justify = |props: Value| match parse_one(json!({"id":1,"type":"Row","props":props,"children":[]})) {
        Node::Row(row) => row.justify,
        other => panic!("{other:?}"),
    };
    assert_eq!(justify(json!({"justify":"center"})), Justify::Center);
    assert_eq!(justify(json!({})), Justify::Start);
}

#[test]
fn a_field_highlights_its_first_option_unless_the_page_sends_auto_highlight_false() {
    let auto = |props: Value| match parse_one(json!({"id":1,"type":"Textarea","props":props,"children":[]})) {
        Node::Input(input) => input.auto_highlight,
        other => panic!("{other:?}"),
    };
    assert!(auto(json!({})));
    assert!(auto(json!({"autoHighlight":true})));
    assert!(auto(json!({"autoHighlight":"no"})));
    assert!(!auto(json!({"autoHighlight":false})));
}

#[test]
fn a_textarea_takes_tab_only_when_the_page_asks() {
    let takes = |props: Value| match parse_one(json!({"id":1,"type":"Textarea","props":props,"children":[]})) {
        Node::Input(input) => input.takes_tab,
        other => panic!("{other:?}"),
    };
    assert!(takes(json!({"takesTab":true})));
    assert!(!takes(json!({"takesTab":false})));
    assert!(!takes(json!({})));
}

#[test]
fn a_dot_has_a_tone_or_is_green() {
    let dot = |props| match parse_one(json!({"id":1,"type":"Dot","props":props,"children":[]}))
    {
        Node::Dot(dot) => dot.tone,
        _ => panic!("a dot"),
    };
    assert_eq!(dot(json!({})), None);
    assert_eq!(dot(json!({"tone":"danger"})), Some(Tone::Danger));
    assert_eq!(dot(json!({"tone":"muted"})), Some(Tone::Muted));
    assert_eq!(dot(json!({"tone":"purple"})), None);
    assert_eq!(dot(json!({"tone":7})), None);
}

#[test]
fn a_dot_can_show_an_agents_state() {
    let state = |props| match parse_one(json!({"id":1,"type":"Dot","props":props,"children":[]})) {
        Node::Dot(dot) => dot.state,
        _ => panic!("a dot"),
    };
    assert_eq!(state(json!({"state":"working"})), Some(crate::agents::AgentState::Working));
    assert_eq!(state(json!({"state":"blocked"})), Some(crate::agents::AgentState::Blocked));
    assert_eq!(state(json!({"state":"asleep"})), None);
}

fn gauge(props: Value, children: Value) -> Gauge {
    match parse_one(json!({"id":1,"type":"Gauge","props":props,"children":children})) {
        Node::Gauge(gauge) => gauge,
        _ => panic!("a gauge"),
    }
}

#[test]
fn a_gauge_has_a_label_a_bounded_percent_levels_and_a_note() {
    let full = gauge(
        json!({"label":"Memory","percent":42.5,"levels":[70,90]}),
        json!([slot("note", json!([text(2, "Resets in 2h")]))]),
    );
    assert_eq!(full.label, "Memory");
    assert_eq!(full.percent, 42.5);
    assert_eq!(full.levels, Some((70.0, 90.0)));
    assert_eq!(plain_text(&full.note), "Resets in 2h");

    // A page that sends nonsense gets a bar that is empty or full, not one that draws outside its box.
    let bounded =
        |percent: Value| gauge(json!({"label":"x","percent":percent}), json!([])).percent;
    assert_eq!(bounded(json!(-5)), 0.0);
    assert_eq!(bounded(json!(1e300)), 100.0);
    assert_eq!(bounded(json!("40")), 0.0);
    assert_eq!(bounded(Value::Null), 0.0);

    let none = gauge(json!({}), json!([]));
    assert_eq!(
        (none.label.as_str(), none.percent, none.levels),
        ("", 0.0, None)
    );
    assert!(none.note.is_empty());
    // Levels are two numbers or nothing.
    for levels in [
        json!([70]),
        json!([1, 2, 3]),
        json!(["a", "b"]),
        json!(70),
        json!([70, null]),
    ] {
        assert_eq!(
            gauge(json!({"levels":levels}), json!([])).levels,
            None,
            "{levels}"
        );
    }
}

#[test]
fn a_gauge_turns_warn_and_danger_at_its_levels() {
    let level = |percent: f64, levels: Value| {
        gauge(json!({"percent":percent,"levels":levels}), json!([])).level()
    };
    assert_eq!(level(40.0, json!([70, 90])), Level::Normal);
    assert_eq!(level(69.0, json!([70, 90])), Level::Normal);
    assert_eq!(level(70.0, json!([70, 90])), Level::Warn);
    assert_eq!(level(85.0, json!([70, 90])), Level::Warn);
    assert_eq!(level(90.0, json!([70, 90])), Level::Danger);
    assert_eq!(level(100.0, json!([70, 90])), Level::Danger);
    assert_eq!(level(99.0, json!(null)), Level::Normal);
}

#[test]
fn a_disclosure_has_a_summary_and_a_body() {
    let Node::Disclosure(disclosure) =
        parse_one(json!({"id":1,"type":"Disclosure","props":{},"children":[
        slot("body", json!([text(2, "details")])),
        slot("summary", json!([text(3, "2 need sign-in")]))]}))
    else {
        panic!("a disclosure")
    };
    assert_eq!(plain_text(&disclosure.summary), "2 need sign-in");
    assert_eq!(plain_text(&disclosure.body), "details");
    assert_eq!(disclosure.id, 1);
}

fn select(props: Value) -> Select {
    match parse_one(json!({"id":1,"type":"Select","props":props,"children":[]})) {
        Node::Select(select) => select,
        _ => panic!("a select"),
    }
}

#[test]
fn chips_can_be_a_segmented_toggle_and_a_select_cannot() {
    let props = json!({"label":"Mode","segmented":true,"options":[{"value":"a"}]});
    let Node::Chips(chips) = parse_one(json!({"id":1,"type":"Chips","props":props,"children":[]})) else {
        panic!("chips")
    };
    assert!(chips.segmented);
    assert!(!select(props).segmented);
}

#[test]
fn a_select_has_options_a_value_and_a_handler() {
    let select = select(
        json!({"label":"Session","value":"b","onChange":"1:onChange","options":[
        {"value":"a","label":"Alpha"},
        {"value":"b","label":"Beta","disabled":true},
        {"value":"c"},
        {"label":"no value"},
        "not an object",
        {"value":3,"label":"not a string"}]}),
    );
    assert_eq!(select.label, "Session");
    assert_eq!(select.on_change.as_deref(), Some("1:onChange"));
    assert_eq!(
        select.options,
        [
            SelectOption {
                value: "a".into(),
                label: "Alpha".into(),
                disabled: false,
                detail: None,
                toggle: None,
            },
            SelectOption {
                value: "b".into(),
                label: "Beta".into(),
                disabled: true,
                detail: None,
                toggle: None,
            },
            // No label is the value, so the option still shows something.
            SelectOption {
                value: "c".into(),
                label: "c".into(),
                disabled: false,
                detail: None,
                toggle: None,
            },
        ]
    );
    assert_eq!((select.current(), select.shown()), (Some("b"), "Beta"));
}

#[test]
fn a_select_option_can_be_a_toggle_with_a_detail_and_a_select_can_carry_a_badge() {
    let chosen = select(json!({"label":"Model","badge":"Ultracode","options":[
        {"value":"a"},
        {"value":"u","label":"Ultracode","detail":"Plans bigger work","toggle":true},
        {"value":"v","toggle":"on"}]}));
    assert_eq!(chosen.badge.as_deref(), Some("Ultracode"));
    assert_eq!(chosen.options[0].toggle, None);
    assert_eq!((chosen.options[1].toggle, chosen.options[1].detail.as_deref()), (Some(true), Some("Plans bigger work")));
    assert_eq!(chosen.options[2].toggle, None);
    assert_eq!(select(json!({"badge":""})).badge, None);
}

/// Tabs of the "line" variant are chips the app underlines; a select has no such look.
#[test]
fn chips_can_be_line_tabs_and_a_select_cannot() {
    let props = json!({"label":"Pull request","line":true,"options":[{"value":"a"}]});
    let Node::Chips(tabs) =
        parse_one(json!({"id":1,"type":"Chips","props":props,"children":[]}))
    else {
        panic!("chips")
    };
    assert!(tabs.line);
    assert!(!select(props).line);
}

#[test]
fn a_select_without_a_value_shows_its_first_option_and_a_value_nobody_has_shows_itself() {
    let options = json!([{"value":"a","label":"Alpha"},{"value":"b","label":"Beta"}]);
    let first = select(json!({"options":options}));
    assert_eq!((first.current(), first.shown()), (Some("a"), "Alpha"));
    let unknown = select(json!({"options":options,"value":"zzz"}));
    assert_eq!(unknown.shown(), "zzz");
    let empty = select(json!({}));
    assert_eq!((empty.current(), empty.shown()), (None, ""));
}

#[test]
fn a_select_is_bounded_and_its_bad_handler_is_none() {
    // One label too long: all of them would be more than a page may send.
    let label = |n: usize| if n == 0 { "x".repeat(MAX_TEXT_CHARS * 2) } else { "x".into() };
    let many: Vec<Value> = (0..MAX_OPTIONS * 2)
        .map(|n| json!({"value":n.to_string(),"label":label(n)}))
        .collect();
    let bounded = select(json!({"options":many,"onChange":"../../etc"}));
    assert_eq!(bounded.options.len(), MAX_OPTIONS);
    assert_eq!(bounded.options[0].label.chars().count(), MAX_TEXT_CHARS);
    assert_eq!(bounded.on_change, None);
}

#[test]
fn sizes_are_bounded() {
    let long = "x".repeat(MAX_TEXT_CHARS * 2);
    let parsed = tree(json!([{"id":1,"type":"#text","text":long}])).unwrap();
    assert_eq!(parsed.text().chars().count(), MAX_TEXT_CHARS);

    let many: Vec<Value> = (0..=MAX_NODES)
        .map(|id| json!({"id":id,"type":"#text","text":"a"}))
        .collect();
    assert_eq!(tree(Value::Array(many)), Err(TreeError::TooManyNodes));

    let mut deep = json!([]);
    for id in 0..=MAX_DEPTH {
        deep = json!([{"id":id,"type":"Stack","props":{},"children":deep}]);
    }
    assert_eq!(tree(deep), Err(TreeError::TooDeep));

    let huge = format!(
        r##"{{"id":0,"type":"#root","children":[{{"id":1,"type":"#text","text":"{}"}}]}}"##,
        "x".repeat(MAX_JSON_BYTES)
    );
    assert_eq!(Tree::parse(&huge), Err(TreeError::TooLarge));
}

/// A long chat came to over 5,000 nodes and its page was refused whole.
#[test]
fn a_page_of_20000_nodes_is_taken_and_one_more_is_refused() {
    let nodes = |count: usize| -> Value {
        Value::Array((0..count).map(|id| json!({"id":id,"type":"#text","text":"a"})).collect())
    };
    assert!(tree(nodes(20_000)).is_ok());
    assert_eq!(tree(nodes(20_001)), Err(TreeError::TooManyNodes));
}

/// A paste into the chat's message box over 20,000 characters lost its tail.
#[test]
fn a_text_field_keeps_200000_characters_of_its_value_and_cuts_the_rest() {
    let value = |len: usize| match parse_one(
        json!({"id":1,"type":"Textarea","props":{"value":"x".repeat(len)},"children":[]}),
    ) {
        Node::Input(input) => input.value.chars().count(),
        other => panic!("{other:?}"),
    };
    assert_eq!(value(200_000), 200_000);
    assert_eq!(value(200_001), 200_000);
}

#[test]
fn what_is_not_a_tree_is_refused() {
    for json in [
        "",
        "[]",
        "null",
        r#"{"id":0,"type":"Page"}"#,
        r##"{"type":"#root"}"##,
    ] {
        assert!(Tree::parse(json).is_err(), "{json}");
    }
}

#[test]
fn graph_windows_preserve_geometry_and_reject_oversized_or_malformed_data() {
    let mut props = serde_json::json!({ "total": 10000, "lanes": 2, "selected": -1, "offset": 5000,
        "rows": [{"hash":"abc", "subject":"x".repeat(3000), "author":"Ada", "date":"2026-11-06", "refs":["H:main"], "lane":1, "color":2, "merge":true, "lines":[0,1,0,2,1,1,1,2]}]
    }).as_object().unwrap().clone();
    let parsed = graph(1, &props).unwrap();
    assert_eq!(
        parsed.rows[0].subject.len(),
        3000,
        "graph data is not generic truncated text"
    );
    assert_eq!(parsed.rows[0].lines, [0, 1, 0, 2, 1, 1, 1, 2]);
    assert!(parsed.rows[0].merge);
    let row = props["rows"][0].clone();
    props.insert("rows".into(), serde_json::json!(vec![row.clone(); 257]));
    assert!(graph(1, &props).is_err());
    props.insert("rows".into(), serde_json::json!([row]));
    props["rows"][0]["lines"] = serde_json::json!([0, 1, 3, 0]);
    assert!(graph(1, &props).is_err());
    props["rows"][0]["lines"] = serde_json::json!([0, 1, 0, 0]);
    props["rows"][0]["subject"] = serde_json::json!("x".repeat(4097));
    assert!(graph(1, &props).is_err());
}
fn svg(props: Value) -> Option<Svg> {
    match tree(json!([{"id":1,"type":"Svg","props":props,"children":[]}]))
        .unwrap()
        .children
        .pop()
    {
        Some(Node::Svg(drawing)) => Some(drawing),
        None => None,
        Some(other) => panic!("an svg, not {other:?}"),
    }
}

#[test]
fn an_svg_is_drawn_from_markup_the_app_writes_from_its_checked_shapes() {
    let drawing = svg(json!({
        "viewBox":"0 0 16 16","size":18,"tone":"warn","label":"Epic",
        "fill":"none","stroke":"currentColor","strokeWidth":"1.4","strokeLinecap":"round",
        "shapes":[
            ["rect",{"x":"2.5","y":2.5,"width":11,"height":"11","rx":2.5}],
            ["path",{"d":"M9 1.8 3.5 9H8l-1 5.2L12.5 7H8Z","fillRule":"evenodd"}],
            ["g",{"opacity":0.3},[["circle",{"cx":8,"cy":8,"r":2}],["line",{"x1":0,"y1":0,"x2":-4,"y2":4}]]],
            ["polygon",{"points":"0,0 4,0 4,4"}]]
    }))
    .unwrap();
    assert_eq!(
        (drawing.size, drawing.tone, drawing.label.as_deref()),
        (18.0, Some(Tone::Warn), Some("Epic"))
    );
    assert_eq!(
        drawing.markup,
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round">"#,
            r#"<rect x="2.5" y="2.5" width="11" height="11" rx="2.5"/>"#,
            r#"<path d="M9 1.8 3.5 9H8l-1 5.2L12.5 7H8Z" fill-rule="evenodd"/>"#,
            r#"<g opacity="0.3"><circle cx="8" cy="8" r="2"/><line x1="0" y1="0" x2="-4" y2="4"/></g>"#,
            r#"<polygon points="0,0 4,0 4,4"/>"#,
            "</svg>"
        )
    );
    // Without a size it is 16 points, and a size past an icon's is cut to 64.
    let plain = |props: Value| svg(props).unwrap();
    assert_eq!(plain(json!({"viewBox":"0 0 24 24","shapes":[]})).size, 16.0);
    assert_eq!(
        plain(json!({"viewBox":"0,0,24,24","size":1e9,"shapes":[]})).size,
        64.0
    );
}

#[test]
fn an_svg_with_anything_the_app_does_not_draw_is_left_out() {
    let with = |shapes: Value| json!({"viewBox":"0 0 16 16","shapes":shapes});
    let mut nested = json!([["path",{"d":"M0 0h1"}]]);
    for _ in 0..5 {
        nested = json!([["g", {}, nested]]);
    }
    let many: Vec<Value> = (0..=svg::MAX_SHAPES)
        .map(|_| json!(["path",{"d":"M0 0h1"}]))
        .collect();
    // At the limits, it is drawn: what is left out below is left out for the one thing wrong with it.
    let mut deepest = json!([["path",{"d":"M0 0h1"}]]);
    for _ in 0..4 {
        deepest = json!([["g", {}, deepest]]);
    }
    let most = Value::Array(many[1..].to_vec());
    for drawn in [
        with(json!([["path",{"d":"M0 0h1"}]])),
        with(deepest),
        with(most),
    ] {
        assert!(svg(drawn.clone()).is_some(), "{drawn}");
    }
    for refused in [
        // Elements that load files, fonts or styles, and attributes outside the allowlist.
        with(json!([["image",{"href":"/etc/passwd","width":16,"height":16}]])),
        with(json!([["text",{"x":0,"y":0},[]]])),
        with(json!([["style", {}]])),
        with(json!([["use",{"href":"#a"}]])),
        with(json!([["path",{"d":"M0 0h1","href":"x"}]])),
        with(json!([["path",{"d":"M0 0h1","style":"fill:red"}]])),
        with(json!([["path",{"d":"M0 0h1","transform":"scale(9)"}]])),
        with(json!([["rect",{"d":"M0 0h1"}]])),
        // Values that aren't what the attribute takes, or would end it.
        with(json!([["path",{"d":"M0 0\" onload=\"x"}]])),
        with(json!([["path",{"d":"M0 0<script>"}]])),
        with(json!([["polygon",{"points":"0,0 M 4,4"}]])),
        with(json!([["path",{"d":"M0 0h1","fill":"red"}]])),
        with(json!([["path",{"d":"M0 0h1","stroke":"url(#paint)"}]])),
        with(json!([["path",{"d":"M0 0h1","strokeLinecap":"pointy"}]])),
        with(json!([["rect",{"width":-1}]])),
        with(json!([["rect",{"width":"10px"}]])),
        with(json!([["rect",{"x":"NaN"}]])),
        with(json!([["rect",{"x":1e9}]])),
        with(json!([["rect",{"x":true}]])),
        with(json!([["g",{"opacity":2}]])),
        // Shapes that aren't shapes, or hold shapes when only a g does.
        with(json!([["path",{"d":"M0 0h1"},[["path",{"d":"M0 0h1"}]]]])),
        with(json!([[7, {}]])),
        with(json!([["path"]])),
        with(json!(["path"])),
        with(json!({"path":{}})),
        json!({"viewBox":"0 0 16 16"}),
        // Over the limits.
        with(Value::Array(many)),
        with(nested),
        with(json!([["path",{"d":"M0 0".repeat(svg::MAX_MARKUP_BYTES)}]])),
        // A view box that isn't four numbers of a square.
        json!({"shapes":[]}),
        json!({"viewBox":"0 0 16","shapes":[]}),
        json!({"viewBox":"0 0 16 8","shapes":[]}),
        json!({"viewBox":"0 0 0 0","shapes":[]}),
        json!({"viewBox":"0 0 1e9 1e9","shapes":[]}),
        json!({"viewBox":"a b c d","shapes":[]}),
        json!({"viewBox":"0 0 16 16","fill":"blue","shapes":[]}),
    ] {
        assert_eq!(svg(refused.clone()), None, "{refused}");
    }
}

#[test]
fn parses_a_split_a_scroll_a_bar_and_a_dot_with_a_colour() {
    let parsed = tree(json!([
        {"id":1,"type":"Split","props":{"defaultSize":320},"children":[
            {"id":2,"type":"Slot","props":{"name":"start"},"children":[
                {"id":3,"type":"Bar","props":{"edge":"bottom","gap":"xs"},"children":[{"id":4,"type":"#text","text":"Head"}]},
                {"id":5,"type":"Scroll","props":{},"children":[{"id":6,"type":"Dot","props":{"color":1122867},"children":[]}]}]},
            {"id":7,"type":"Slot","props":{"name":"end"},"children":[{"id":8,"type":"Dot","props":{"color":99999999},"children":[]}]}]}
    ]))
    .unwrap();
    let Node::Split(split) = &parsed.children[0] else {
        panic!("a split")
    };
    assert_eq!(split.size, 320.0);
    let (Node::Bar(bar), Node::Scroll(scroll)) = (&split.start[0], &split.start[1]) else {
        panic!("a bar over a scroll")
    };
    assert_eq!((bar.edge, bar.gap), (Edge::Bottom, Gap::Xs));
    assert!(matches!(
        &scroll.children[0],
        Node::Dot(Dot {
            color: Some(0x112233),
            ..
        })
    ));
    // A colour that isn't one is no colour.
    assert!(matches!(&split.end[0], Node::Dot(Dot { color: None, .. })));
    // The fixed pane is the start one, always shown, unless the page says otherwise.
    assert!(!split.fixed_end && split.collapse_below.is_none() && !split.pinned);
}

#[test]
fn parses_a_split_whose_fixed_pane_is_on_the_right_and_folds_when_narrow() {
    let parsed = tree(json!([
        {"id":1,"type":"Split","props":{"defaultSize":260,"side":"end","collapseBelow":1100,"pinned":true},"children":[]},
        {"id":2,"type":"Split","props":{"side":"start","collapseBelow":-5,"gap":"l"},"children":[]},
        {"id":3,"type":"Split","props":{"side":"end","collapseBelow":"wide","gap":"huge"},"children":[]}
    ]))
    .unwrap();
    let splits: Vec<&Split> = parsed
        .children
        .iter()
        .map(|node| match node {
            Node::Split(split) => split,
            _ => panic!("a split"),
        })
        .collect();
    assert_eq!(
        (splits[0].size, splits[0].fixed_end, splits[0].collapse_below, splits[0].pinned),
        (260.0, true, Some(1100.0), true)
    );
    // Only "end" moves the pane, and a threshold that is no positive number is none.
    assert_eq!((splits[1].fixed_end, splits[1].collapse_below), (false, None));
    assert_eq!((splits[2].fixed_end, splits[2].collapse_below), (true, None));
    // Room at the divider only when asked for, and a gap it doesn't know is none.
    assert_eq!((splits[0].gap, splits[1].gap, splits[2].gap), (Gap::None, Gap::L, Gap::None));
}

#[test]
fn parses_a_dialog_with_its_title_width_and_close_and_bounds_its_width() {
    let parsed = tree(json!([
        {"id":1,"type":"Dialog","props":{"title":"Plugins","width":640,"onClose":"1:onClose"},"children":[
            {"id":2,"type":"Text","props":{},"children":[{"id":3,"type":"#text","text":"Inside"}]}]},
        {"id":4,"type":"Dialog","props":{"width":99999,"onClose":"no handler"},"children":[]},
        {"id":5,"type":"Dialog","props":{"width":-3},"children":[]}
    ]))
    .unwrap();
    let dialogs: Vec<&Dialog> = parsed
        .children
        .iter()
        .map(|node| match node {
            Node::Dialog(dialog) => dialog,
            _ => panic!("a dialog"),
        })
        .collect();
    assert_eq!(
        (dialogs[0].title.as_str(), dialogs[0].width, dialogs[0].on_close.as_deref()),
        ("Plugins", 640.0, Some("1:onClose"))
    );
    assert_eq!(plain_text(&parsed.children[..1]), "Inside");
    // A width past the bounds is held to them, and a close that isn't a handler is none.
    assert_eq!((dialogs[1].width, dialogs[1].on_close.as_deref()), (1_000.0, None));
    assert_eq!(dialogs[2].width, 280.0);
}

#[test]
fn parses_a_terminal_and_a_text_that_asks_for_links() {
    let parsed = tree(json!([
        {"id":1,"type":"Terminal","props":{"pane":"12","height":5000}},
        {"id":2,"type":"Terminal","props":{"pane":"x".repeat(21)}},
        {"id":6,"type":"Terminal","props":{"pane":"3","fill":true}},
        {"id":3,"type":"Text","props":{"links":true},"children":[{"id":4,"type":"#text","text":"See ABC-12"}]},
        {"id":5,"type":"Stack","props":{"links":true},"children":[]}
    ]))
    .unwrap();
    assert_eq!(parsed.children[0], Node::Terminal(Terminal { id: 1, pane: "12".into(), height: 800.0, fill: false }));
    assert_eq!(parsed.children[1], Node::Terminal(Terminal { id: 2, pane: String::new(), height: 220.0, fill: false }));
    assert_eq!(parsed.children[2], Node::Terminal(Terminal { id: 6, pane: "3".into(), height: 220.0, fill: true }));
    assert!(matches!(&parsed.children[3], Node::Text(text) if text.links));
    assert!(matches!(&parsed.children[4], Node::Stack(stack) if stack.links));
}

#[test]
fn a_sheet_has_a_title_a_description_a_header_and_a_body_and_hears_its_close() {
    let parsed = tree(json!([{"id":1,"type":"Sheet","props":{"onClose":"1:onClose"},"children":[
        slot("title", json!([{"id":3,"type":"#text","text":"Past sessions"}])),
        slot("description", json!([{"id":4,"type":"#text","text":"In shop."}])),
        slot("header", json!([{"id":5,"type":"SearchField","props":{"label":"Search"}}])),
        text(6, "Fix the checkout"),
        text(7, "Split the cart")
    ]}]))
    .unwrap();
    let Node::Sheet(sheet) = &parsed.children[0] else { panic!("a sheet") };
    assert_eq!(sheet.on_close.as_deref(), Some("1:onClose"));
    assert_eq!((plain_text(&sheet.title), plain_text(&sheet.description)), ("Past sessions".into(), "In shop.".into()));
    assert!(matches!(sheet.header.as_slice(), [Node::SearchField(_)]));
    assert_eq!(sheet.body.len(), 2);
    // What a reader would see has its title, so a page that waits on it finds it.
    assert_eq!(parsed.text(), "Past sessions In shop. Fix the checkoutSplit the cart");
}

/// A shortcut the app or a text field answers to is dropped, so the page's button can't take it away from them.
#[test]
fn a_buttons_shortcut_is_kept_only_when_no_app_or_text_key_has_it() {
    let shortcut = |value: &str| {
        let parsed = tree(json!([button(json!({"shortcut": value, "onClick": "2:onClick"}))])).unwrap();
        let Node::Button(button) = &parsed.children[0] else { panic!("a button") };
        button.shortcut.clone()
    };
    let kept = shortcut("Cmd-Shift-P").expect("a free shortcut is kept");
    assert_eq!((kept.canonical(), kept.aria()), ("shift-cmd-p".into(), "Shift+Meta+P".into()));
    assert!(shortcut("cmd-alt-b").is_some() && shortcut("cmd-shift-[").is_some());
    for refused in [
        // The app's: quit, close, new tab, settings, tab by number, font size, the sidebar, the palette.
        "cmd-q", "cmd-w", "cmd-t", "cmd-n", "cmd-,", "cmd-3", "cmd-=", "cmd--", "cmd-0", "cmd-b", "cmd-k",
        "cmd-shift-d", "ctrl-shift-t",
        // A text field's.
        "cmd-a", "cmd-z", "cmd-shift-z",
        // Plain keys and keys that move a caret belong to what has the keyboard.
        "p", "shift-p", "cmd-left", "cmd-enter", "cmd-backspace",
        // Not a key.
        "cmd-", "cmd-shift", "hyper-p", "",
    ] {
        assert_eq!(shortcut(refused), None, "{refused:?} was kept");
    }
}

#[test]
fn a_page_that_sizes_its_own_text_names_its_handler() {
    let parsed = tree(json!([{"id":1,"type":"Page","props":{"onTextScale":"1:onTextScale"},"children":[]}])).unwrap();
    let Node::Page(page) = &parsed.children[0] else { panic!("a page") };
    assert_eq!(page.on_text_scale.as_deref(), Some("1:onTextScale"));
}

#[test]
fn a_page_asks_for_the_pixels_or_a_gradient_behind_it_and_any_other_backdrop_is_its_plain_ground() {
    let backdrop = |props: Value| {
        let parsed = tree(json!([{"id":1,"type":"Page","props":props,"children":[]}])).unwrap();
        let Node::Page(page) = &parsed.children[0] else { panic!("a page") };
        page.backdrop
    };
    use crate::pixels::Pattern;
    assert_eq!(backdrop(json!({"backdrop":"pixels"})), Some(Backdrop::Pixels(Pattern::Drift)));
    assert_eq!(backdrop(json!({"backdrop":"pixels:rain"})), Some(Backdrop::Pixels(Pattern::Rain)));
    assert_eq!(backdrop(json!({"backdrop":"pixels:checker"})), Some(Backdrop::Pixels(Pattern::Checker)));
    assert_eq!(backdrop(json!({"backdrop":"pixels:embers"})), Some(Backdrop::Pixels(Pattern::Embers)));
    // A pattern from a newer plugin than this Wings knows is the drift, not the plain ground.
    assert_eq!(backdrop(json!({"backdrop":"pixels:snow"})), Some(Backdrop::Pixels(Pattern::Drift)));
    for (name, pattern) in [
        ("mono", Pattern::Mono),
        ("ocean", Pattern::Ocean),
        ("meadow", Pattern::Meadow),
        ("dusk", Pattern::Dusk),
        ("blocks", Pattern::Blocks),
        ("bayer", Pattern::Bayer),
        ("grain", Pattern::Grain),
        ("grain-lagoon", Pattern::GrainLagoon),
    ] {
        assert_eq!(backdrop(json!({"backdrop": format!("pixels:{name}")})), Some(Backdrop::Pixels(pattern)));
    }
    // A page of an older plugin that still asks for the dropped moire gets the plain ground, not a gradient.
    assert_eq!(backdrop(json!({"backdrop":"gradient:moire"})), None);
    assert_eq!(backdrop(json!({"backdrop":"gradient:halo"})), Some(Backdrop::Gradient(crate::gradients::Preset::Halo)));
    assert_eq!(
        backdrop(json!({"backdrop":"gradient:nighty-night"})),
        Some(Backdrop::Gradient(crate::gradients::Preset::NightyNight))
    );
    assert_eq!(backdrop(json!({"backdrop":"gradient:aurora"})), None);
    assert_eq!(backdrop(json!({"backdrop":"gradient:"})), None);
    assert_eq!(backdrop(json!({"backdrop":"halo"})), None);
    assert_eq!(backdrop(json!({"backdrop":"glow"})), None);
    assert_eq!(backdrop(json!({"backdrop":3})), None);
    assert_eq!(backdrop(json!({})), None);
}

fn slot_text(id: u64, name: &str, text: &str) -> Value {
    json!({"id":id,"type":"Slot","props":{"name":name},"children":[{"id":id + 1,"type":"#text","text":text}]})
}

/// The row a `ListRow` lays out: the lead's box, then the title over the detail, then what trails.
fn row_line(node: &Node) -> &Row {
    let children = match node {
        Node::ListButton(item) => &item.button.children,
        Node::Stack(stack) => &stack.children,
        _ => panic!("a list button or a stack"),
    };
    let [Node::Row(line)] = children.as_slice() else { panic!("one row") };
    line
}

#[test]
fn a_list_row_with_a_click_is_a_list_button_and_one_without_sits_in_its_inset() {
    let lead = json!({"id":20,"type":"Slot","props":{"name":"lead"},"children":[{"id":21,"type":"Spinner","props":{}}]});
    let trailing = json!({"id":30,"type":"Slot","props":{"name":"trailing"},"children":[button(json!({}))]});
    let parsed = tree(json!([
        {"id":1,"type":"ListRow","props":{"onClick":"1:onClick","title":"Open it"},"children":[lead, slot_text(10, "title", "Build"), slot_text(12, "detail", "Done · 2s"), trailing]},
        {"id":40,"type":"ListRow","props":{},"children":[slot_text(41, "title", "Earlier")]},
        // A disabled one takes no click: it is drawn as one without.
        {"id":50,"type":"ListRow","props":{"onClick":"50:onClick","disabled":true},"children":[slot_text(51, "title", "Off")]},
    ]))
    .unwrap();
    let Node::ListButton(clickable) = &parsed.children[0] else { panic!("a list button") };
    assert_eq!((clickable.button.on_click.as_deref(), clickable.button.title.as_deref()), (Some("1:onClick"), Some("Open it")));
    let line = row_line(&parsed.children[0]);
    assert!(line.grow, "it takes the button's width, so what trails sits at its end");
    let [Node::Stack(lead), Node::Stack(words), Node::Button(_)] = line.children.as_slice() else { panic!("lead, words, trailing: {line:?}") };
    assert_eq!(lead.width, Some(16.0));
    assert!(words.grow);
    assert_eq!(plain_text(&words.children), "BuildDone · 2s");
    // Beside a control the words keep part of the row, and a control too wide for the rest goes under them.
    assert!(line.wrap && words.min_share == Some(0.4), "{line:?}");
    for plain in &parsed.children[1..] {
        let Node::Stack(inset) = plain else { panic!("a stack") };
        assert!(inset.inset && !inset.grow);
        // No lead: the title is the row's first part, at the inset's edge.
        assert!(matches!(row_line(plain).children.as_slice(), [Node::Stack(words)] if words.width.is_none() && words.min_share.is_none()));
        assert!(!row_line(plain).wrap, "nothing trails, so nothing goes under");
    }
}

/// Rows that are one `Card` have an edge of their own, so the heading and description line up with it.
#[test]
fn a_section_of_one_card_lines_its_heading_up_with_the_card() {
    let parsed = tree(json!([{"id":1,"type":"Section","props":{},"children":[
        slot_text(2, "title", "Entries"),
        slot_text(4, "description", "What ran."),
        {"id":6,"type":"Card","props":{},"children":[]},
    ]}]))
    .unwrap();
    let Node::Stack(section) = &parsed.children[0] else { panic!("a stack") };
    let [Node::Stack(head), Node::Stack(about), Node::Stack(rows)] = section.children.as_slice() else { panic!("{section:?}") };
    assert!(!head.inset && !about.inset);
    assert!(matches!(rows.children.as_slice(), [Node::Card(_)]));
}

#[test]
fn a_section_puts_its_heading_and_description_in_the_rows_inset_and_its_rows_under_them() {
    let parsed = tree(json!([{"id":1,"type":"Section","props":{},"children":[
        slot_text(2, "title", "Subagents"),
        slot_text(4, "description", "What this session started."),
        {"id":6,"type":"ListRow","props":{"onClick":"6:onClick"},"children":[slot_text(7, "title", "Explore")]},
    ]}]))
    .unwrap();
    let Node::Stack(section) = &parsed.children[0] else { panic!("a stack") };
    assert_eq!((section.id, section.gap), (1, Gap::Xs));
    let [Node::Stack(head), Node::Stack(about), Node::Stack(rows)] = section.children.as_slice() else { panic!("{section:?}") };
    assert!(head.inset && matches!(head.children.as_slice(), [Node::Heading(heading)] if plain_text(&heading.children) == "Subagents"));
    assert!(about.inset && about.measure.is_some() && about.max_width.is_none(), "read at a measure, not centred");
    assert!(matches!(about.children.as_slice(), [Node::Text(text)] if text.tone == Some(Tone::Muted) && text.size == TextSize::Small));
    assert_eq!(rows.gap, Gap::None);
    assert!(matches!(rows.children.as_slice(), [Node::ListButton(_)]));
    // Without a description there is no line for one.
    let bare = tree(json!([{"id":1,"type":"Section","props":{},"children":[slot_text(2, "title", "Folders")]}])).unwrap();
    let Node::Stack(bare) = &bare.children[0] else { panic!("a stack") };
    assert_eq!(bare.children.len(), 2);
}

#[test]
fn a_collapsible_section_is_a_disclosure_with_its_heading_as_the_summary_and_its_rows_as_the_body() {
    let parsed = tree(json!([{"id":1,"type":"Section","props":{"collapsible":true,"open":false,"onOpenChange":"1:onOpenChange"},"children":[
        slot_text(2, "title", "Subagents · 4 running"),
        slot_text(4, "description", "What this session started."),
        {"id":6,"type":"ListRow","props":{},"children":[slot_text(7, "title", "Explore")]},
    ]}]))
    .unwrap();
    let Node::Disclosure(fold) = &parsed.children[0] else { panic!("{:?}", parsed.children) };
    assert_eq!((fold.id, fold.open, fold.section), (1, Some(false), true));
    assert_eq!(fold.on_open_change.as_deref(), Some("1:onOpenChange"));
    assert_eq!(plain_text(&fold.summary), "Subagents · 4 running");
    assert_eq!(fold.body.len(), 2, "the description, then the rows");
    // Without the flag the section is the plain stack, as an older page draws it.
    let plain = tree(json!([{"id":1,"type":"Section","props":{"open":false},"children":[slot_text(2, "title", "Folders")]}])).unwrap();
    assert!(matches!(plain.children[0], Node::Stack(_)));
}

#[test]
fn actions_put_the_dismissing_buttons_first_and_ghost_whatever_variant_they_came_in() {
    let parsed = tree(json!([{"id":1,"type":"Actions","props":{},"children":[
        {"id":5,"type":"Button","props":{"variant":"default"},"children":[{"id":6,"type":"#text","text":"Review"}]},
        {"id":3,"type":"Slot","props":{"name":"dismiss"},"children":[button(json!({"variant":"outline"}))]},
    ]}]))
    .unwrap();
    let Node::Actions(actions) = &parsed.children[0] else { panic!("actions") };
    let variants: Vec<_> = actions
        .children
        .iter()
        .map(|node| match node {
            Node::Button(button) => (plain_text(&button.children), button.variant),
            _ => panic!("a button"),
        })
        .collect();
    assert_eq!(variants, [("Go".to_owned(), Variant::Ghost), ("Review".to_owned(), Variant::Default)]);
}
