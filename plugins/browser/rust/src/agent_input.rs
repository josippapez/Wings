//! The real input the agent tools send a Chromium page (`wings_plugin::ui::web_input`): a click, a hover, a drag, typed text
//! and a key as the events the browser takes from a person's mouse and keyboard, so a page sees them as trusted. These are
//! plain functions of what the tool was given; `agent_page.rs` finds where an element is (`window.__wingsAgent.locate`) and
//! sends them.

use wings_plugin::ui::WebInput;

/// How many moves a drag makes between the press and the release, so a page that follows the pointer sees it travel.
const DRAG_STEPS: u32 = 5;

const ALT: u32 = 1;
const CTRL: u32 = 2;
const META: u32 = 4;
const SHIFT: u32 = 8;

fn mouse(kind: &'static str, x: f64, y: f64, held: bool, count: u32) -> WebInput {
    let button = if held || kind != "mouseMoved" { "left" } else { "none" };
    WebInput::Mouse { kind, x, y, button, buttons: u32::from(held), click_count: count, modifiers: 0 }
}

/// The pointer comes to the point, with no button: what `hover` does.
pub fn hover(x: f64, y: f64) -> Vec<WebInput> {
    vec![mouse("mouseMoved", x, y, false, 0)]
}

/// A left click: the pointer comes, presses and lets go.
pub fn click(x: f64, y: f64) -> Vec<WebInput> {
    vec![mouse("mouseMoved", x, y, false, 0), mouse("mousePressed", x, y, true, 1), mouse("mouseReleased", x, y, false, 1)]
}

/// The pointer presses at `from`, moves to `to` in steps with the button down, and lets go there.
pub fn drag(from: (f64, f64), to: (f64, f64)) -> Vec<WebInput> {
    let mut events = vec![mouse("mouseMoved", from.0, from.1, false, 0), mouse("mousePressed", from.0, from.1, true, 1)];
    for step in 1..=DRAG_STEPS {
        let at = f64::from(step) / f64::from(DRAG_STEPS);
        events.push(mouse("mouseMoved", from.0 + (to.0 - from.0) * at, from.1 + (to.1 - from.1) * at, true, 0));
    }
    events.push(mouse("mouseReleased", to.0, to.1, false, 1));
    events
}

/// `text` typed into what has the focus, in one go.
pub fn type_text(text: &str) -> Result<Vec<WebInput>, String> {
    if text.is_empty() {
        return Err("type takes text".into());
    }
    Ok(vec![WebInput::Text(text.to_owned())])
}

/// A field's whole content replaced by `value`: the text typed over what is selected, or, for none, the selection deleted.
pub fn replace(value: &str) -> Vec<WebInput> {
    if value.is_empty() {
        return key_press("Backspace", 0).unwrap_or_default();
    }
    vec![WebInput::Text(value.to_owned())]
}

/// A key as the keyboard names it: its `key`, `code`, Windows virtual key code, and the text it types.
fn named(name: &str) -> Option<(&'static str, &'static str, u32, Option<&'static str>)> {
    Some(match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => ("Enter", "Enter", 13, Some("\r")),
        "tab" => ("Tab", "Tab", 9, None),
        "escape" | "esc" => ("Escape", "Escape", 27, None),
        "backspace" => ("Backspace", "Backspace", 8, None),
        "delete" | "del" => ("Delete", "Delete", 46, None),
        "space" | " " => (" ", "Space", 32, Some(" ")),
        "arrowleft" | "left" => ("ArrowLeft", "ArrowLeft", 37, None),
        "arrowup" | "up" => ("ArrowUp", "ArrowUp", 38, None),
        "arrowright" | "right" => ("ArrowRight", "ArrowRight", 39, None),
        "arrowdown" | "down" => ("ArrowDown", "ArrowDown", 40, None),
        "home" => ("Home", "Home", 36, None),
        "end" => ("End", "End", 35, None),
        "pageup" => ("PageUp", "PageUp", 33, None),
        "pagedown" => ("PageDown", "PageDown", 34, None),
        "insert" => ("Insert", "Insert", 45, None),
        _ => return None,
    })
}

const FUNCTION_KEYS: [(&str, &str); 12] = [("F1", "F1"), ("F2", "F2"), ("F3", "F3"), ("F4", "F4"), ("F5", "F5"), ("F6", "F6"), ("F7", "F7"), ("F8", "F8"), ("F9", "F9"), ("F10", "F10"), ("F11", "F11"), ("F12", "F12")];

/// The editing command a shortcut with Ctrl or Meta stands for: macOS's text fields take it as a command, not as the key.
/// Copy, cut and paste are none: they reach the person's own clipboard, and Wings takes no such command from an agent.
fn command(key: &str, modifiers: u32) -> Option<&'static str> {
    match key.to_ascii_lowercase().as_str() {
        "a" => Some("selectAll"),
        "z" if modifiers & SHIFT != 0 => Some("redo"),
        "z" => Some("undo"),
        _ => None,
    }
}

/// The modifier keys a combo holds, in the order they are pressed.
const HELD: [(u32, &str, &str, u32); 4] = [(CTRL, "Control", "ControlLeft", 17), (SHIFT, "Shift", "ShiftLeft", 16), (ALT, "Alt", "AltLeft", 18), (META, "Meta", "MetaLeft", 91)];

fn key_event(kind: &'static str, key: &str, code: &str, vk: u32, text: Option<&str>, modifiers: u32, commands: Vec<&'static str>) -> WebInput {
    WebInput::Key { kind, key: key.to_owned(), code: code.to_owned(), vk, text: text.map(str::to_owned), modifiers, commands }
}

/// A key or combo as a person types it (`Enter`, `Shift+Tab`, `Control+a`, `Meta+Shift+z`, `a`, `F5`) as key events: each
/// modifier goes down, then the key goes down and up, then the modifiers come up. A key that types something sends its text
/// unless Ctrl, Alt or Meta is held, which makes it a shortcut.
pub fn press(combo: &str) -> Result<Vec<WebInput>, String> {
    let mut parts: Vec<&str> = combo.split('+').collect();
    let key = match parts.pop() {
        Some("") if parts.last().is_some_and(|last| last.is_empty()) => {
            parts.pop();
            "+"
        }
        Some("") | None => return Err("press takes a key, like Enter, Shift+Tab or Control+a".into()),
        Some(key) => key,
    };
    let mut modifiers = 0;
    for part in parts {
        modifiers |= match part.to_ascii_lowercase().as_str() {
            "control" | "ctrl" => CTRL,
            "shift" => SHIFT,
            "alt" | "option" => ALT,
            "meta" | "cmd" | "command" => META,
            other => return Err(format!("{other} isn't a modifier: use Control, Shift, Alt or Meta")),
        };
    }
    key_press(key, modifiers)
}

fn key_press(key: &str, modifiers: u32) -> Result<Vec<WebInput>, String> {
    let shortcut = modifiers & (CTRL | ALT | META) != 0;
    let mut chars = key.chars();
    let (key, code, vk, text, commands): (String, String, u32, Option<String>, Vec<&'static str>) = if let Some((key, code, vk, text)) = named(key) {
        // Wings refuses these (`tree::web_input_event`): they reach the person's clipboard even with no command.
        if (vk == 45 && modifiers & (CTRL | SHIFT) != 0) || (vk == 46 && modifiers & SHIFT != 0) {
            return Err("Shift+Insert, Control+Insert and Shift+Delete use your clipboard, which agents don't get: type the text instead".into());
        }
        (key.to_owned(), code.to_owned(), vk, text.map(str::to_owned), Vec::new())
    } else if let Some((name, code)) = FUNCTION_KEYS.iter().find(|(name, _)| name.eq_ignore_ascii_case(key)) {
        let number: u32 = name[1..].parse().unwrap_or(1);
        ((*name).to_owned(), (*code).to_owned(), 111 + number, None, Vec::new())
    } else if let (Some(ch), None) = (chars.next(), chars.next()) {
        let shown = if modifiers & SHIFT != 0 { ch.to_uppercase().to_string() } else { ch.to_string() };
        let (code, vk) = match ch {
            'a'..='z' | 'A'..='Z' => (format!("Key{}", ch.to_ascii_uppercase()), ch.to_ascii_uppercase() as u32),
            '0'..='9' => (format!("Digit{ch}"), ch as u32),
            _ => (String::new(), 0),
        };
        let commands = if modifiers & (CTRL | META) != 0 { command(&shown, modifiers).into_iter().collect() } else { Vec::new() };
        (shown.clone(), code, vk, Some(shown), commands)
    } else {
        return Err(format!("{key} isn't a key press knows: use a character, a name like Enter or ArrowDown, or F1 to F12"));
    };
    let text = text.filter(|_| !shortcut);
    let (down, mut up): (&'static str, Vec<WebInput>) = (if text.is_some() { "keyDown" } else { "rawKeyDown" }, Vec::new());
    let mut events = Vec::new();
    let mut held = 0;
    for (bit, name, name_code, name_vk) in HELD {
        if modifiers & bit != 0 {
            let before = held;
            held |= bit;
            events.push(key_event("rawKeyDown", name, name_code, name_vk, None, held, Vec::new()));
            up.push(key_event("keyUp", name, name_code, name_vk, None, before, Vec::new()));
        }
    }
    events.push(key_event(down, &key, &code, vk, text.as_deref(), modifiers, commands));
    events.push(key_event("keyUp", &key, &code, vk, None, modifiers, Vec::new()));
    up.reverse();
    events.extend(up);
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(events: &[WebInput]) -> Vec<String> {
        events
            .iter()
            .map(|event| match event {
                WebInput::Mouse { kind, .. } => (*kind).to_owned(),
                WebInput::Key { kind, key, .. } => format!("{kind}:{key}"),
                WebInput::Text(text) => format!("text:{text}"),
            })
            .collect()
    }

    /// A click is the pointer coming, pressing and letting go at the element's centre, once each, as a person's is.
    #[test]
    fn a_click_moves_presses_and_releases_at_one_point_with_a_click_count_of_one() {
        let events = click(40.5, 22.0);
        assert_eq!(kinds(&events), ["mouseMoved", "mousePressed", "mouseReleased"]);
        for event in &events {
            let WebInput::Mouse { x, y, .. } = event else { panic!("{event:?}") };
            assert_eq!((*x, *y), (40.5, 22.0));
        }
        assert_eq!(
            events[1],
            WebInput::Mouse { kind: "mousePressed", x: 40.5, y: 22.0, button: "left", buttons: 1, click_count: 1, modifiers: 0 }
        );
        assert_eq!(events[2], WebInput::Mouse { kind: "mouseReleased", x: 40.5, y: 22.0, button: "left", buttons: 0, click_count: 1, modifiers: 0 });
        assert_eq!(hover(1.0, 2.0), [WebInput::Mouse { kind: "mouseMoved", x: 1.0, y: 2.0, button: "none", buttons: 0, click_count: 0, modifiers: 0 }]);
    }

    /// A drag keeps the button down while it moves from one point to the other and lets go on the target.
    #[test]
    fn a_drag_presses_at_the_source_moves_with_the_button_down_and_releases_on_the_target() {
        let events = drag((10.0, 10.0), (60.0, 110.0));
        assert_eq!(events.len(), 2 + DRAG_STEPS as usize + 1);
        assert_eq!(kinds(&events)[..2], ["mouseMoved", "mousePressed"]);
        assert_eq!(kinds(&events).last().map(String::as_str), Some("mouseReleased"));
        let moves: Vec<(f64, f64, u32)> = events[2..events.len() - 1].iter().map(|event| match event { WebInput::Mouse { x, y, buttons, .. } => (*x, *y, *buttons), other => panic!("{other:?}") }).collect();
        assert!(moves.iter().all(|(_, _, buttons)| *buttons == 1), "the button is held");
        assert_eq!(moves.last().map(|(x, y, _)| (*x, *y)), Some((60.0, 110.0)));
        assert!(moves.windows(2).all(|pair| pair[0].0 < pair[1].0 && pair[0].1 < pair[1].1), "it travels");
        assert_eq!(events.last(), Some(&WebInput::Mouse { kind: "mouseReleased", x: 60.0, y: 110.0, button: "left", buttons: 0, click_count: 1, modifiers: 0 }));
    }

    #[test]
    fn typed_text_is_inserted_whole_and_a_fill_types_over_the_selection_or_deletes_it() {
        assert_eq!(type_text("héllo wörld"), Ok(vec![WebInput::Text("héllo wörld".into())]));
        assert!(type_text("").is_err());
        assert_eq!(replace("new"), [WebInput::Text("new".into())]);
        assert_eq!(kinds(&replace("")), ["rawKeyDown:Backspace", "keyUp:Backspace"], "an empty value deletes what fill selected");
    }

    /// A key that types sends its text on the way down; a named key or a shortcut sends none, and the editing shortcuts
    /// carry their command, which macOS's fields need.
    #[test]
    fn a_key_goes_down_and_up_with_its_text_only_when_it_types() {
        assert_eq!(kinds(&press("Enter").unwrap()), ["keyDown:Enter", "keyUp:Enter"]);
        let enter = press("Enter").unwrap();
        assert_eq!(enter[0], WebInput::Key { kind: "keyDown", key: "Enter".into(), code: "Enter".into(), vk: 13, text: Some("\r".into()), modifiers: 0, commands: vec![] });
        assert_eq!(kinds(&press("ArrowDown").unwrap()), ["rawKeyDown:ArrowDown", "keyUp:ArrowDown"]);
        assert_eq!(press("down"), press("ArrowDown"));
        let letter = press("a").unwrap();
        assert!(matches!(&letter[0], WebInput::Key { kind: "keyDown", key, code, vk: 65, text: Some(text), .. } if key == "a" && code == "KeyA" && text == "a"));
        assert!(matches!(&press("5").unwrap()[0], WebInput::Key { code, vk: 53, .. } if code == "Digit5"));
        assert!(matches!(&press("F5").unwrap()[0], WebInput::Key { kind: "rawKeyDown", key, vk: 116, text: None, .. } if key == "F5"));
    }

    #[test]
    fn modifiers_go_down_before_the_key_and_up_after_it_with_the_right_bits() {
        let events = press("Control+Shift+a").unwrap();
        assert_eq!(kinds(&events), ["rawKeyDown:Control", "rawKeyDown:Shift", "rawKeyDown:A", "keyUp:A", "keyUp:Shift", "keyUp:Control"]);
        let bits: Vec<u32> = events.iter().map(|event| match event { WebInput::Key { modifiers, .. } => *modifiers, other => panic!("{other:?}") }).collect();
        assert_eq!(bits, [2, 10, 10, 10, 2, 0]);
        let WebInput::Key { text, commands, .. } = &events[2] else { panic!() };
        assert_eq!((text.as_deref(), commands.as_slice()), (None, &["selectAll"][..]), "a shortcut types nothing and carries its command");
        let WebInput::Key { commands, modifiers, .. } = &press("Meta+Shift+z").unwrap()[2] else { panic!() };
        assert_eq!((commands.as_slice(), *modifiers), (&["redo"][..], 12));
        let WebInput::Key { kind, text, .. } = &press("Shift+Tab").unwrap()[1] else { panic!() };
        assert_eq!((*kind, text.as_deref()), ("rawKeyDown", None));
        let WebInput::Key { kind, key, text, .. } = &press("Shift+a").unwrap()[1] else { panic!() };
        assert_eq!((*kind, key.as_str(), text.as_deref()), ("keyDown", "A", Some("A")), "shift alone still types");
        assert!(matches!(&press("Control++").unwrap()[1], WebInput::Key { key, .. } if key == "+"));
        for combo in ["Shift+Insert", "Control+Insert", "Shift+Delete"] {
            assert!(press(combo).unwrap_err().contains("clipboard"), "{combo}");
        }
        assert!(press("Insert").is_ok() && press("Delete").is_ok() && press("Control+Delete").is_ok());
        // Copy, cut and paste carry no command: the person's clipboard isn't the agent's.
        for combo in ["Meta+c", "Meta+x", "Meta+v", "Control+v"] {
            let WebInput::Key { commands, .. } = &press(combo).unwrap()[1] else { panic!() };
            assert!(commands.is_empty(), "{combo}");
        }
    }

    #[test]
    fn a_key_press_does_not_know_is_an_error_that_says_so() {
        for bad in ["", "Hyper+a", "Control+", "Enterr", "Control+Enterr"] {
            assert!(press(bad).is_err(), "{bad}");
        }
        assert!(press("Hyper+a").unwrap_err().contains("hyper"));
    }
}
