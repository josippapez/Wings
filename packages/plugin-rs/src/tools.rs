//! The tools Claude, or another plugin, calls: `wings-plugin-sdk/tools`' `registerTools`. A tool's input is checked
//! against its schema before its handler runs, so a wrong one goes back as an error that names the field to fix and
//! the handler only ever sees an input that fits. The schema is the JSON Schema the manifest's `contributes.mcpTools`
//! has for the tool, of the kinds `s` builds: `object`, `string` (with `minLength`, `maxLength`, `format` and `enum`),
//! `integer`, `number`, `boolean` and `array`.

use crate::{host, task};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap, future::Future, pin::Pin, rc::Rc};

type Run = Rc<dyn Fn(Value, Value) -> Pin<Box<dyn Future<Output = Result<Value, String>>>>>;

thread_local! {
    static TOOLS: RefCell<HashMap<String, (Value, Run)>> = RefCell::new(HashMap::new());
}

/// Answers calls of the tool `name` with `run`, which gets the input and `{ paneId, project, session }` of who called.
/// What it returns goes back as text: a string as it is, null as "Done", anything else as JSON. An error goes back as
/// a failed call with its message.
///
/// A tool whose schema says `pattern` answers every call with an error that says so, and never runs: there is nothing
/// here to check a pattern with, and a field that looked checked and wasn't would be worse.
pub fn register<F, Fut>(name: &str, schema: Value, run: F)
where
    F: Fn(Value, Value) -> Fut + 'static,
    Fut: Future<Output = Result<Value, String>> + 'static,
{
    let first = TOOLS.with(|tools| {
        let mut tools = tools.borrow_mut();
        let first = tools.is_empty();
        tools.insert(name.to_owned(), (schema, Rc::new(move |input, context| Box::pin(run(input, context)))));
        first
    });
    if first {
        host::on("tool", |token, call| {
            let call = call.clone();
            task::spawn(async move {
                let (text, is_error) = match answer(&call).await {
                    Ok(text) => (text, false),
                    Err(text) => (text, true),
                };
                drop(host::call("toolDone", json!({ "token": token, "text": text, "isError": is_error })));
            });
        });
    }
}

fn has_pattern(schema: &Value) -> bool {
    match schema {
        Value::Object(map) => map.contains_key("pattern") || map.values().any(has_pattern),
        Value::Array(list) => list.iter().any(has_pattern),
        _ => false,
    }
}

async fn answer(call: &Value) -> Result<String, String> {
    // A Claude Code hook: this SDK has none, which says nothing, as a JS plugin without that hook does.
    if call["hook"] == true {
        return Ok("{}".into());
    }
    let name = call["name"].as_str().unwrap_or_default();
    let Some((schema, run)) = TOOLS.with(|tools| tools.borrow().get(name).cloned()) else {
        return Err(format!("The plugin has no handler for {name}"));
    };
    let input = match &call["arguments"] {
        Value::Null => json!({}),
        input => input.clone(),
    };
    if has_pattern(&schema) {
        return Err(format!("{name}'s schema has a pattern, which a plugin written in Rust can't check yet. Tell the plugin's author."));
    }
    check(&schema, &input, "", name)?;
    let context = json!({ "paneId": call["paneId"], "project": call["project"], "session": call["session"] });
    Ok(match run(input, context).await? {
        Value::String(text) => text,
        Value::Null => "Done".into(),
        other => serde_json::to_string_pretty(&other).unwrap_or_default(),
    })
}

/// A whole number with a comma between its thousands, as the JS SDK's messages write it.
fn num(n: f64) -> String {
    if n.fract() != 0.0 || n.abs() >= 1e15 {
        return n.to_string();
    }
    let digits = (n.abs() as u64).to_string();
    let mut out = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    if n < 0.0 { format!("-{out}") } else { out }
}

fn digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// A YYYY-MM-DD that is a day on the calendar, so 2026-02-30 isn't one.
fn is_day(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' || !digits(&text[..4]) || !digits(&text[5..7]) || !digits(&text[8..]) {
        return false;
    }
    let (year, month, day): (u32, u32, u32) = (text[..4].parse().unwrap(), text[5..7].parse().unwrap(), text[8..].parse().unwrap());
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(at, byte)| if matches!(at, 8 | 13 | 18 | 23) { byte == b'-' } else { byte.is_ascii_hexdigit() })
}

/// Two digits no more than `most`.
fn two(text: &str, most: u32) -> bool {
    text.len() == 2 && digits(text) && text.parse::<u32>().is_ok_and(|n| n <= most)
}

/// A date, `T`, a time to the second with an optional fraction, and `Z` or an offset: 2026-10-08T09:00:00+02:00.
fn is_date_time(text: &str) -> bool {
    if !text.is_ascii() || text.len() < 20 || !is_day(&text[..10]) || !matches!(text.as_bytes()[10], b'T' | b't') {
        return false;
    }
    let rest = &text[11..];
    let Some(zone) = rest.find(['Z', 'z', '+', '-']) else {
        return false;
    };
    let (time, zone) = rest.split_at(zone);
    let (clock, fraction) = time.split_once('.').map_or((time, None), |(clock, fraction)| (clock, Some(fraction)));
    let mut parts = clock.split(':');
    let clock_ok = matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(hours), Some(minutes), Some(seconds), None) if two(hours, 23) && two(minutes, 59) && two(seconds, 60)
    );
    let zone_ok = matches!(zone, "Z" | "z")
        || zone[1..].split_once(':').is_some_and(|(hours, minutes)| two(hours, 23) && two(minutes, 59));
    clock_ok && fraction.is_none_or(digits) && zone_ok
}

fn kind(name: &str) -> &'static str {
    match name {
        "string" => "text",
        "integer" => "a whole number",
        "number" => "a number",
        "boolean" => "true or false",
        "array" => "a list",
        _ => "an object",
    }
}

/// An error that names the field and what it allows, unless `value` fits `schema`. `path` is where the value sits,
/// like `status` or `keys[1]`, and `owner` what an unknown field is said not to belong to: the tool at the top. The
/// messages are the JS SDK's.
pub fn check(schema: &Value, value: &Value, path: &str, owner: &str) -> Result<(), String> {
    if let Some(allowed) = schema["enum"].as_array()
        && !allowed.contains(value)
    {
        let names: Vec<_> = allowed.iter().map(|one| one.as_str().map_or_else(|| one.to_string(), str::to_owned)).collect();
        return Err(format!("{path} must be one of {}.", names.join(", ")));
    }
    let wanted = schema["type"].as_str().unwrap_or("object");
    let fits = match wanted {
        "string" => value.is_string(),
        "integer" => value.as_f64().is_some_and(|n| n.fract() == 0.0),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        _ => value.is_object(),
    };
    if !fits {
        return Err(if path.is_empty() { "The input must be an object.".into() } else { format!("{path} must be {}.", kind(wanted)) });
    }
    match value {
        Value::String(text) => {
            // JSON Schema counts characters, so an emoji is one.
            let length = text.chars().count() as f64;
            if let Some(least) = schema["minLength"].as_f64()
                && length < least
            {
                let s = if least == 1.0 { "" } else { "s" };
                return Err(format!("{path} needs at least {} character{s}, this one has {}.", num(least), num(length)));
            }
            if let Some(most) = schema["maxLength"].as_f64()
                && length > most
            {
                return Err(format!("{path} is limited to {} characters, this one has {}.", num(most), num(length)));
            }
            let format = match schema["format"].as_str() {
                Some("uuid") => Some((is_uuid(text), "a uuid, like 123e4567-e89b-12d3-a456-426614174000")),
                Some("date") => Some((is_day(text), "a date as YYYY-MM-DD")),
                Some("date-time") => Some((is_date_time(text), "a date and time like 2026-10-08T09:00:00+02:00")),
                _ => None,
            };
            if let Some((false, is)) = format {
                return Err(format!("{path} must be {is}, this one is {value}."));
            }
        }
        Value::Number(number) => {
            let n = number.as_f64().unwrap_or(0.0);
            if let Some(least) = schema["minimum"].as_f64()
                && n < least
            {
                return Err(format!("{path} must be at least {}, this one is {}.", num(least), num(n)));
            }
            if let Some(most) = schema["maximum"].as_f64()
                && n > most
            {
                return Err(format!("{path} must be at most {}, this one is {}.", num(most), num(n)));
            }
        }
        Value::Array(items) => {
            if let Some(most) = schema["maxItems"].as_f64()
                && items.len() as f64 > most
            {
                return Err(format!("{path} takes up to {} items, this one has {}.", num(most), num(items.len() as f64)));
            }
            for (at, item) in items.iter().enumerate() {
                let place = format!("{path}[{at}]");
                check(&schema["items"], item, &place, &place)?;
            }
        }
        Value::Object(fields) => {
            let at = |name: &str| if path.is_empty() { name.to_owned() } else { format!("{path}.{name}") };
            let none = serde_json::Map::new();
            let properties = schema["properties"].as_object().unwrap_or(&none);
            let stray: Vec<_> = fields.keys().filter(|name| !properties.contains_key(*name)).map(|name| at(name)).collect();
            if !stray.is_empty() {
                let names: Vec<_> = properties.keys().map(String::as_str).collect();
                let takes = if names.is_empty() { "no input".to_owned() } else { names.join(", ") };
                let fields = if stray.len() > 1 { "fields" } else { "field" };
                return Err(format!("Unknown {fields}: {}. {owner} takes {takes}.", stray.join(", ")));
            }
            for (name, property) in properties {
                match fields.get(name) {
                    Some(value) => check(property, value, &at(name), &at(name))?,
                    None if schema["required"].as_array().is_some_and(|required| required.iter().any(|one| one == name)) => {
                        return Err(format!("{} is required.", at(name)));
                    }
                    None => {}
                }
            }
        }
        _ => {}
    }
    Ok(())
}
