//! What an MCP server asks you in the middle of a turn (Claude Code's `elicitation` control request): a message with a form
//! to fill, or a link to open. Claude Code waits for the answer, so the chat shows a card for it and answers once, from
//! your click: Submit (or Open and continue for a link), Decline or Cancel.
//!
//! The form is the server's `requested_schema`: an object of string, number, integer, boolean and choice fields. A field of
//! another shape makes it a card that can only be declined or cancelled.

use std::collections::HashMap;

use serde_json::{Map, Value, json};
use wings_plugin::format::truncate;

use crate::session::{Item, Session};

/// How much of the server's message the card keeps.
const MESSAGE_KEPT: usize = 2_000;

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Text,
    Number { whole: bool },
    Bool,
    /// The values the server accepts, each with the label it is shown by.
    Choice(Vec<(String, String)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub key: String,
    pub title: String,
    pub description: String,
    pub required: bool,
    pub input: Input,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Form(Vec<Field>),
    /// A link the server wants you to open, which is https.
    Url(String),
    /// Something this page can't show, and why: it can only be declined or cancelled.
    Other(String),
}

/// How you answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Accept,
    Decline,
    Cancel,
}

impl Action {
    fn word(self) -> &'static str {
        match self {
            Action::Accept => "accept",
            Action::Decline => "decline",
            Action::Cancel => "cancel",
        }
    }
}

#[derive(Debug)]
pub struct Elicitation {
    /// The request id Claude Code knows it by, which the answer carries.
    pub request: String,
    pub server: String,
    pub message: String,
    pub kind: Kind,
    /// What you typed or picked, by field: the text as typed, `true` or `false` for a box, the value of a choice.
    pub values: HashMap<String, String>,
}

impl Elicitation {
    /// Reads the `control_request` line. None when it has no request id to answer under.
    pub fn parse(line: &Value) -> Option<Elicitation> {
        let request = line["request_id"].as_str()?.to_owned();
        let body = &line["request"];
        let kind = match body["mode"].as_str().unwrap_or("form") {
            "form" => match fields(&body["requested_schema"], &body["property_order"]) {
                Ok(fields) => Kind::Form(fields),
                Err(why) => Kind::Other(why),
            },
            "url" => match body["url"].as_str().filter(|url| url.starts_with("https://")) {
                Some(url) => Kind::Url(url.to_owned()),
                None => Kind::Other(format!("It points at {}, which isn't an https link, so it isn't opened here.", body["url"].as_str().unwrap_or("nothing"))),
            },
            other => Kind::Other(format!("It asks in a way Wings doesn't know ({other}).")),
        };
        let values = match &kind {
            Kind::Form(fields) => defaults(fields, &body["requested_schema"]["properties"]),
            _ => HashMap::new(),
        };
        Some(Elicitation {
            request,
            server: body["mcp_server_name"].as_str().unwrap_or("An MCP server").to_owned(),
            message: truncate(body["message"].as_str().unwrap_or_default(), MESSAGE_KEPT),
            kind,
            values,
        })
    }

    /// What you have in the field `key`.
    pub fn value(&self, key: &str) -> &str {
        self.values.get(key).map_or("", String::as_str)
    }

    /// What Submit sends: the filled fields as an object, or why they aren't ready.
    pub fn content(&self) -> Result<Value, String> {
        let Kind::Form(fields) = &self.kind else {
            return Err("There is no form to submit.".to_owned());
        };
        let mut content = Map::new();
        for field in fields {
            let said = self.value(&field.key).trim();
            let value = match &field.input {
                Input::Bool => Some(json!(said == "true")),
                _ if said.is_empty() => None,
                Input::Text => Some(json!(said)),
                Input::Number { whole } => Some(number(said, *whole).ok_or_else(|| format!("{} must be {}.", field.title, if *whole { "a whole number" } else { "a number" }))?),
                Input::Choice(options) => {
                    options.iter().find(|(value, _)| value == said).ok_or_else(|| format!("Pick one of the choices for {}.", field.title))?;
                    Some(json!(said))
                }
            };
            match value {
                Some(value) => {
                    content.insert(field.key.clone(), value);
                }
                None if field.required => return Err(format!("{} is required.", field.title)),
                None => {}
            }
        }
        Ok(Value::Object(content))
    }

    /// The answer's content for `action`: the form's for Submit, none for the rest and for a link.
    fn answer_content(&self, action: Action) -> Result<Option<Value>, String> {
        match (action, &self.kind) {
            (Action::Accept, Kind::Form(_)) => self.content().map(Some),
            (Action::Accept, Kind::Url(_)) => Ok(None),
            (Action::Accept, Kind::Other(_)) => Err("It can only be declined or cancelled here.".to_owned()),
            _ => Ok(None),
        }
    }

    /// The link to open, for a link.
    pub fn url(&self) -> Option<&str> {
        match &self.kind {
            Kind::Url(url) => Some(url),
            _ => None,
        }
    }

    /// What the conversation keeps of the exchange once it is answered: the title and the lines.
    fn note(&self, action: Action, content: Option<&Value>) -> Item {
        let said = match (action, content) {
            (Action::Accept, Some(Value::Object(content))) if !content.is_empty() => {
                let lines: Vec<String> = content.iter().map(|(key, value)| format!("{key}: {}", value.as_str().map_or_else(|| value.to_string(), str::to_owned))).collect();
                format!("You answered:\n{}", lines.join("\n"))
            }
            (Action::Accept, _) => "You accepted.".to_owned(),
            (Action::Decline, _) => "You declined.".to_owned(),
            (Action::Cancel, _) => "You cancelled.".to_owned(),
        };
        Item::Note(format!("{} asked: {}", self.server, truncate(self.message.lines().next().unwrap_or_default(), 60)), said)
    }
}

/// The fields of a form's schema, in the order the server gave, else by name. Why it can't be shown when one is of a shape
/// this page has no control for.
fn fields(schema: &Value, order: &Value) -> Result<Vec<Field>, String> {
    let properties = schema["properties"].as_object().ok_or("It has no fields to fill.")?;
    let mut keys: Vec<&str> = order.as_array().into_iter().flatten().filter_map(Value::as_str).filter(|key| properties.contains_key(*key)).collect();
    keys.extend(properties.keys().map(String::as_str).filter(|key| !keys.contains(key)).collect::<Vec<_>>());
    let required: Vec<&str> = schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    keys.into_iter()
        .map(|key| -> Result<Field, String> {
            let property = &properties[key];
            let title = property["title"].as_str().filter(|title| !title.is_empty()).unwrap_or(key).to_owned();
            let input = input_of(property).ok_or_else(|| format!("It asks for “{title}” in a form Wings can't show."))?;
            Ok(Field { key: key.to_owned(), description: property["description"].as_str().unwrap_or_default().to_owned(), required: required.contains(&key), title, input })
        })
        .collect()
}

/// The control a property takes, none for a shape this page has no control for.
fn input_of(property: &Value) -> Option<Input> {
    // A choice is a list of values, or of `const` and `title`; `enumNames` is the older way to label the values.
    let choices: Option<Vec<(String, String)>> = if let Some(values) = property["enum"].as_array() {
        let names = property["enumNames"].as_array();
        values
            .iter()
            .enumerate()
            .map(|(at, value)| {
                let value = value.as_str()?;
                Some((value.to_owned(), names.and_then(|names| names.get(at)?.as_str()).unwrap_or(value).to_owned()))
            })
            .collect()
    } else if let Some(options) = property["oneOf"].as_array() {
        options.iter().map(|option| Some((option["const"].as_str()?.to_owned(), option["title"].as_str().or(option["const"].as_str())?.to_owned()))).collect()
    } else {
        None
    };
    if property["enum"].is_array() || property["oneOf"].is_array() {
        return choices.map(Input::Choice);
    }
    match property["type"].as_str()? {
        "string" => Some(Input::Text),
        "number" => Some(Input::Number { whole: false }),
        "integer" => Some(Input::Number { whole: true }),
        "boolean" => Some(Input::Bool),
        _ => None,
    }
}

/// What the form starts with: the schema's defaults.
fn defaults(fields: &[Field], properties: &Value) -> HashMap<String, String> {
    fields
        .iter()
        .filter_map(|field| {
            let default = &properties[field.key.as_str()]["default"];
            let said = match default {
                Value::String(said) => said.clone(),
                Value::Number(number) => number.to_string(),
                Value::Bool(on) => on.to_string(),
                _ => return None,
            };
            Some((field.key.clone(), said))
        })
        .collect()
}

/// A number as typed: whole, or with a fraction unless `whole`.
fn number(said: &str, whole: bool) -> Option<Value> {
    if let Ok(n) = said.parse::<i64>() {
        return Some(json!(n));
    }
    let n: f64 = said.parse().ok().filter(|n: &f64| n.is_finite() && !whole)?;
    Some(json!(n))
}

/// The line that answers the request `request`.
fn answer_line(request: &str, action: Action, content: Option<Value>) -> String {
    let mut response = json!({ "action": action.word() });
    if let Some(content) = content {
        response["content"] = content;
    }
    format!("{}\n", json!({ "type": "control_response", "response": { "subtype": "success", "request_id": request, "response": response } }))
}

impl Session {
    /// The line that answers what the server asks with `action`, which takes the card away and leaves a note of it in the
    /// conversation. For Submit the form's fields go with it; None while they aren't ready, which the card says, and when
    /// there is no such question waiting.
    pub fn answer_elicitation(&mut self, request: &str, action: Action) -> Option<String> {
        let at = self.ops.elicitations.iter().position(|asked| asked.request == request)?;
        let content = self.ops.elicitations[at].answer_content(action).ok()?;
        let asked = self.ops.elicitations.remove(at);
        self.items.push(asked.note(action, content.as_ref()));
        Some(answer_line(request, action, content))
    }

    /// Takes the question away when Claude Code no longer needs the answer: the turn was stopped, or the server gave up.
    pub fn withdraw_elicitation(&mut self, request: &str) {
        self.ops.elicitations.retain(|asked| asked.request != request);
    }

    /// You typed or picked `value` in the field `key` of the question `request`.
    pub fn edit_elicitation(&mut self, request: &str, key: &str, value: String) {
        if let Some(asked) = self.ops.elicitations.iter_mut().find(|asked| asked.request == request) {
            asked.values.insert(key.to_owned(), value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What claude 2.1.327 printed for a form (read from its output), and for a link.
    fn form() -> Value {
        json!({ "type": "control_request", "request_id": "31ad4bf7", "request": { "subtype": "elicitation", "mcp_server_name": "elicit", "message": "Pick a flavour and count", "mode": "form",
            "requested_schema": { "type": "object", "properties": {
                "flavour": { "type": "string", "title": "Flavour", "enum": ["vanilla", "mint"] },
                "count": { "type": "number", "title": "Count" },
                "note": { "type": "string", "title": "Note", "description": "Anything else" },
                "hot": { "type": "boolean", "title": "Hot" } }, "required": ["flavour"] },
            "property_order": ["flavour", "count", "note", "hot"] } })
    }

    fn link() -> Value {
        json!({ "type": "control_request", "request_id": "7e11", "request": { "subtype": "elicitation", "mcp_server_name": "elicit", "message": "Sign in to continue", "mode": "url", "url": "https://example.com/login", "elicitation_id": "abc" } })
    }

    fn session_with(line: Value) -> Session {
        let mut session = Session::default();
        session.hear(&line);
        session
    }

    fn sent(line: &str) -> Value {
        serde_json::from_str(line.trim()).unwrap()
    }

    #[test]
    fn a_form_has_its_fields_in_the_servers_order_with_their_controls() {
        let asked = Elicitation::parse(&form()).expect("a question");
        assert_eq!((asked.request.as_str(), asked.server.as_str(), asked.message.as_str()), ("31ad4bf7", "elicit", "Pick a flavour and count"));
        let Kind::Form(fields) = &asked.kind else { panic!("a form") };
        let shown: Vec<(&str, &Input, bool)> = fields.iter().map(|field| (field.key.as_str(), &field.input, field.required)).collect();
        assert_eq!(
            shown,
            [
                ("flavour", &Input::Choice(vec![("vanilla".into(), "vanilla".into()), ("mint".into(), "mint".into())]), true),
                ("count", &Input::Number { whole: false }, false),
                ("note", &Input::Text, false),
                ("hot", &Input::Bool, false),
            ]
        );
        assert_eq!(fields[2].description, "Anything else");
    }

    #[test]
    fn submit_sends_the_fields_filled_as_the_server_asked_for_them() {
        let mut session = session_with(form());
        for (key, value) in [("flavour", "mint"), ("count", "3"), ("hot", "true"), ("note", " hi ")] {
            session.edit_elicitation("31ad4bf7", key, value.into());
        }
        let line = sent(&session.answer_elicitation("31ad4bf7", Action::Accept).expect("an answer"));
        // The shape claude 2.1.327 handed its MCP server unchanged.
        assert_eq!(
            line,
            json!({ "type": "control_response", "response": { "subtype": "success", "request_id": "31ad4bf7",
                "response": { "action": "accept", "content": { "flavour": "mint", "count": 3, "hot": true, "note": "hi" } } } })
        );
        assert!(session.ops.elicitations.is_empty());
        let Some(Item::Note(title, said)) = session.items.last() else { panic!("a note") };
        assert_eq!(title, "elicit asked: Pick a flavour and count");
        assert!(said.starts_with("You answered:") && said.contains("flavour: mint") && said.contains("count: 3"));
    }

    #[test]
    fn submit_waits_for_a_required_field_and_for_numbers_that_are_numbers() {
        let mut session = session_with(form());
        assert_eq!(session.answer_elicitation("31ad4bf7", Action::Accept), None);
        assert_eq!(session.ops.elicitations[0].content(), Err("Flavour is required.".to_owned()));
        session.edit_elicitation("31ad4bf7", "flavour", "mint".into());
        session.edit_elicitation("31ad4bf7", "count", "three".into());
        assert_eq!(session.ops.elicitations[0].content(), Err("Count must be a number.".to_owned()));
        session.edit_elicitation("31ad4bf7", "count", "2.5".into());
        assert_eq!(session.ops.elicitations[0].content().unwrap()["count"], json!(2.5));
        session.edit_elicitation("31ad4bf7", "flavour", "chocolate".into());
        assert_eq!(session.ops.elicitations[0].content(), Err("Pick one of the choices for Flavour.".to_owned()));
        // Nothing was sent, so the card is still there.
        assert_eq!(session.ops.elicitations.len(), 1);
    }

    #[test]
    fn declining_and_cancelling_send_no_content_and_need_no_fields() {
        for (action, word) in [(Action::Decline, "decline"), (Action::Cancel, "cancel")] {
            let mut session = session_with(form());
            let line = sent(&session.answer_elicitation("31ad4bf7", action).expect("an answer"));
            assert_eq!(line["response"], json!({ "subtype": "success", "request_id": "31ad4bf7", "response": { "action": word } }));
            assert!(session.ops.elicitations.is_empty());
            // One answer a question: a second click on a card that is gone sends nothing.
            assert_eq!(session.answer_elicitation("31ad4bf7", action), None);
        }
    }

    #[test]
    fn a_link_is_accepted_without_content_and_only_when_it_is_https() {
        let mut session = session_with(link());
        assert_eq!(session.ops.elicitations[0].url(), Some("https://example.com/login"));
        let line = sent(&session.answer_elicitation("7e11", Action::Accept).expect("an answer"));
        assert_eq!(line["response"]["response"], json!({ "action": "accept" }));
        let mut plain = link();
        plain["request"]["url"] = json!("http://example.com/login");
        let mut session = session_with(plain);
        assert_eq!(session.ops.elicitations[0].url(), None);
        assert_eq!(session.answer_elicitation("7e11", Action::Accept), None);
        assert!(session.answer_elicitation("7e11", Action::Decline).is_some());
    }

    #[test]
    fn a_form_with_a_field_this_page_cant_show_can_still_be_declined() {
        let mut odd = form();
        odd["request"]["requested_schema"]["properties"]["tags"] = json!({ "type": "array", "title": "Tags", "items": { "type": "string" } });
        odd["request"].as_object_mut().unwrap().remove("property_order");
        let mut session = session_with(odd);
        assert!(matches!(&session.ops.elicitations[0].kind, Kind::Other(why) if why.contains("Tags")));
        assert_eq!(session.answer_elicitation("31ad4bf7", Action::Accept), None);
        assert!(session.answer_elicitation("31ad4bf7", Action::Decline).is_some());
    }

    #[test]
    fn choices_labels_and_defaults_come_from_the_schema() {
        let mut line = form();
        line["request"]["requested_schema"]["properties"] = json!({
            "size": { "type": "string", "oneOf": [{ "const": "s", "title": "Small" }, { "const": "l", "title": "Large" }], "default": "l" },
            "colour": { "type": "string", "enum": ["r", "g"], "enumNames": ["Red", "Green"] },
            "age": { "type": "integer", "default": 7 },
            "agree": { "type": "boolean", "default": true } });
        line["request"].as_object_mut().unwrap().remove("property_order");
        line["request"]["requested_schema"]["required"] = json!([]);
        let asked = Elicitation::parse(&line).unwrap();
        let Kind::Form(fields) = &asked.kind else { panic!("a form") };
        // Without an order they are by name.
        assert_eq!(fields.iter().map(|field| field.key.as_str()).collect::<Vec<_>>(), ["age", "agree", "colour", "size"]);
        assert_eq!(fields[2].input, Input::Choice(vec![("r".into(), "Red".into()), ("g".into(), "Green".into())]));
        assert_eq!(fields[3].input, Input::Choice(vec![("s".into(), "Small".into()), ("l".into(), "Large".into())]));
        assert_eq!(asked.content().unwrap(), json!({ "age": 7, "agree": true, "size": "l" }));
        let mut asked = asked;
        asked.values.insert("age".into(), "7.5".into());
        assert_eq!(asked.content(), Err("age must be a whole number.".to_owned()));
    }

    #[test]
    fn the_server_withdrawing_or_a_stopped_turn_takes_the_card_away() {
        let mut session = session_with(form());
        session.hear(&json!({ "type": "control_cancel_request", "request_id": "31ad4bf7" }));
        assert!(session.ops.elicitations.is_empty());
    }

    #[test]
    fn a_question_tells_you_once_and_isnt_a_message_of_yours() {
        let session = session_with(form());
        assert_eq!(session.alerts.len(), 1);
        assert_eq!(session.alerts[0].0, "elicit asks you");
        assert!(session.items.is_empty());
    }
}
