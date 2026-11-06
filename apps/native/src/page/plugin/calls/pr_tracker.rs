//! The calls PR tracker makes that the plugins ported before it didn't: HTTP requests and the secrets they send, writes
//! through the signed-in CLIs, messages to an agent's prompt, the plugin's settings page and events for other plugins.
//! These are the desktop's `fetch`, `secret*`, `post`, `sendToAgent`, `openSettings` and `emit` cases in
//! `apps/desktop/src/lib/plugins.ts`, with `plugin_fetch`, `plugin_secret_*` and `plugin_post` in `src-tauri/src/lib.rs`,
//! their limits and their messages. What each may do is `wings-plugins`' to say.

use std::time::Duration;

use serde_json::{Value, json};
use wings_plugins::{
    fetch::{
        declares_secret, fetch_header_ok, fetch_needs_json, fetch_url, may_send_secret,
        valid_secret_name,
    },
    permissions::may_send_to_agents,
    post::{PostRequest, post},
};

use super::{Access, Shared, js_string, to_json};
use crate::page::runtime::Reply;

/// A secret's value, in bytes.
const SECRET_MAX: usize = 8192;
/// A message to an agent, in bytes.
const MESSAGE_MAX: usize = 8 * 1024;
/// An event's JSON, in bytes.
const EVENT_MAX: usize = 16 * 1024;

/// `value` as the desktop's `str(value, max)`: a string cut to `max` characters, or nothing.
fn short(value: Option<&Value>, max: usize) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(|text| text.chars().take(max).collect())
}

fn fetch(shared: &Shared, access: &Access, params: &Value) -> Result<String, String> {
    let plugin = &access.plugin;
    let id = &plugin.manifest.id;
    let asked = js_string(params.get("url"));
    let method = short(params.get("method"), 10).unwrap_or_else(|| "GET".into());
    let method = method.to_uppercase();
    let body = params.get("body").and_then(Value::as_str);
    let url = fetch_url(plugin, &method, &asked, body)
        .ok_or_else(|| format!("{id} may not {method} {asked}"))?;
    let mut builder = ureq::http::Request::builder()
        .method(method.as_str())
        .uri(&url);
    let json_only = fetch_needs_json(plugin, &method, &url);
    if let Some(Value::Object(headers)) = params.get("headers") {
        for (name, value) in headers {
            let value = js_string(Some(value));
            if !fetch_header_ok(name) {
                return Err(format!("{id} may not set the {name} header"));
            }
            if json_only && name.eq_ignore_ascii_case("content-type") {
                if !value.trim().eq_ignore_ascii_case("application/json") {
                    return Err(format!("{id} must send this body as application/json"));
                }
                continue;
            }
            builder = builder.header(name.as_str(), value);
        }
    }
    if json_only {
        builder = builder.header("Content-Type", "application/json");
    }
    if let Some(name) = short(params.get("bearer"), 64) {
        if !may_send_secret(plugin, &name, &url) {
            return Err(format!("{id} may not send the {name} secret to {url}"));
        }
        let token = access
            .env
            .secret_get(id, &shared.config.scope, &name)?
            .ok_or_else(|| format!("no secret named {name}"))?;
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }
    // No redirects, so a response can't send the request, and its token, to another host.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    let request = builder
        .body(body.unwrap_or_default().to_owned())
        .map_err(|error| error.to_string())?;
    let mut response = agent
        .run(request)
        .map_err(|error| format!("Couldn't reach {asked}: {error}"))?;
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let text = if params.get("binary") == Some(&Value::Bool(true)) {
        base64(
            &response
                .body_mut()
                .read_to_vec()
                .map_err(|error| error.to_string())?,
        )
    } else {
        response
            .body_mut()
            .read_to_string()
            .map_err(|error| error.to_string())?
    };
    to_json(&json!({
        "status": response.status().as_u16(),
        "contentType": content_type,
        "body": text,
    }))
}

/// The standard base64 of `bytes`, with padding: how a `binary` fetch hands over a body that isn't text.
fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let group = chunk.iter().enumerate().fold(0u32, |group, (at, byte)| {
            group | (u32::from(*byte) << (16 - 8 * at))
        });
        for at in 0..4 {
            out.push(if at <= chunk.len() {
                DIGITS[((group >> (18 - 6 * at)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

fn secret_set(shared: &Shared, access: &Access, params: &Value) -> Result<String, String> {
    let id = &access.plugin.manifest.id;
    let name = js_string(params.get("name"));
    let value = js_string(params.get("value"));
    if !declares_secret(&access.plugin, &name) {
        return Err(format!(
            "{id} has no secret named {name} in permissions.secrets"
        ));
    }
    if !valid_secret_name(&name) || value.is_empty() || value.len() > SECRET_MAX {
        return Err(
            "A secret needs a name of letters, digits, - and _, and a value up to 8 KB".into(),
        );
    }
    access
        .env
        .secret_set(id, &shared.config.scope, &name, &value)?;
    Ok("null".into())
}

fn secret_delete(shared: &Shared, access: &Access, params: &Value) -> Result<String, String> {
    let name = js_string(params.get("name"));
    access
        .env
        .secret_delete(&access.plugin.manifest.id, &shared.config.scope, &name)?;
    Ok("null".into())
}

fn secret_has(shared: &Shared, access: &Access, params: &Value) -> Result<String, String> {
    let name = js_string(params.get("name"));
    let kept = access
        .env
        .secret_get(&access.plugin.manifest.id, &shared.config.scope, &name)?;
    to_json(&kept.is_some())
}

fn post_call(access: &Access, params: &Value) -> Result<String, String> {
    let fields = match params.get("fields") {
        Some(fields @ Value::Object(_)) => fields.clone(),
        _ => json!({}),
    };
    let request: PostRequest = serde_json::from_value(json!({
        "program": js_string(params.get("program")),
        "url": js_string(params.get("url")),
        "host": params.get("host").and_then(Value::as_str),
        "fields": fields,
    }))
    .map_err(|error| error.to_string())?;
    to_json(&post(&access.plugin, &request)?)
}

/// What the desktop's wrapper checks of a message before the permission: it has text, is short, and can't drive the
/// agent's prompt, since an escape in a paste could end it early. Newlines and tabs are text.
fn message(params: &Value) -> Result<String, String> {
    let text = params
        .get("text")
        .and_then(Value::as_str)
        .map_or("", str::trim);
    if text.is_empty() {
        return Err("sendToAgent needs a message".into());
    }
    if text.len() > MESSAGE_MAX {
        return Err("A message is up to 8 KB".into());
    }
    if text
        .chars()
        .any(|c| matches!(c, '\0'..='\u{8}' | '\u{b}'..='\u{1f}' | '\u{7f}'))
    {
        return Err("A message can't hold control characters other than newlines and tabs".into());
    }
    Ok(text.to_owned())
}

fn send_to_agent(access: &Access, params: &Value, reply: Reply) {
    let plugin = &access.plugin;
    let pane = js_string(params.get("paneId"));
    let checked = message(params).and_then(|text| {
        if may_send_to_agents(plugin) {
            Ok(text)
        } else {
            Err(format!(
                "{} may not send messages to agents",
                plugin.manifest.id
            ))
        }
    });
    match checked {
        Ok(text) => access.env.send_to_agent(
            &pane,
            &format!(
                "(Message from the {} plugin, sent through Wings.)\n{text}",
                plugin.manifest.name
            ),
            reply,
        ),
        Err(error) => reply.err(error),
    }
}

fn open_settings(access: &Access) -> Result<String, String> {
    let plugin = &access.plugin;
    if plugin.manifest.contributes.settings.is_none() {
        return Err(format!(
            "{} has no settings page. Add contributes.settings to its manifest",
            plugin.manifest.id
        ));
    }
    access.env.open_settings(&plugin.manifest.id)?;
    Ok("null".into())
}

fn emit(access: &Access, params: &Value) -> Result<String, String> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let valid = !name.is_empty()
        && name.len() <= 64
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid {
        return Err("emit takes an event name of lowercase letters, digits and dashes".into());
    }
    let data = params.get("data").unwrap_or(&Value::Null);
    if data.to_string().len() > EVENT_MAX {
        return Err("emit data is up to 16 KB of JSON".into());
    }
    access.env.emit(&access.plugin.manifest.id, name, data);
    Ok("null".into())
}

impl Shared {
    /// The calls of this module, answered through `reply`. The ones that wait for a network or a CLI run on a thread
    /// of their own.
    pub(super) fn forge_call(
        self: &std::sync::Arc<Self>,
        method: &str,
        params: Value,
        reply: Reply,
        access: Access,
    ) {
        let this = self.clone();
        match method {
            "fetch" => self.off_thread(reply, move |_| fetch(&this, &access, &params)),
            "post" => self.off_thread(reply, move |_| post_call(&access, &params)),
            "secretSet" => reply.done(secret_set(self, &access, &params)),
            "secretDelete" => reply.done(secret_delete(self, &access, &params)),
            "secretHas" => self.off_thread(reply, move |_| secret_has(&this, &access, &params)),
            "sendToAgent" => send_to_agent(&access, &params, reply),
            "openSettings" => reply.done(open_settings(&access)),
            "emit" => reply.done(emit(&access, &params)),
            _ => reply.err(format!("unknown method {method}")),
        }
    }
}

#[cfg(test)]
mod tests;
