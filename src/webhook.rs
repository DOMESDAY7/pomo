//! Outgoing webhooks. Every service wants its own payload shape, so each
//! `Kind` knows how to build its request. Requests are kept "simple" (CORS
//! safelisted content types, no custom headers) whenever possible so the
//! browser sends them without a preflight.

use crate::i18n::tr;
use serde::{Deserialize, Serialize};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Kind {
    Discord,
    Slack,
    Ntfy,
    Gotify,
    Custom,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::Discord, Kind::Slack, Kind::Ntfy, Kind::Gotify, Kind::Custom];

    pub fn id(self) -> &'static str {
        match self {
            Kind::Discord => "discord",
            Kind::Slack => "slack",
            Kind::Ntfy => "ntfy",
            Kind::Gotify => "gotify",
            Kind::Custom => "custom",
        }
    }

    pub fn from_id(id: &str) -> Kind {
        Kind::ALL.into_iter().find(|k| k.id() == id).unwrap_or(Kind::Custom)
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Discord => "Discord",
            Kind::Slack => "Slack / Mattermost",
            Kind::Ntfy => "ntfy",
            Kind::Gotify => "Gotify",
            Kind::Custom => tr().custom,
        }
    }

    pub fn placeholder(self) -> &'static str {
        match self {
            Kind::Discord => "https://discord.com/api/webhooks/…",
            Kind::Slack => "https://hooks.slack.com/services/…",
            Kind::Ntfy => "https://ntfy.sh/mon-topic",
            Kind::Gotify => "https://gotify.exemple.fr/message?token=…",
            Kind::Custom => "https://…",
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Webhook {
    pub id: u32,
    pub kind: Kind,
    pub enabled: bool,
    pub url: String,
    // Only used by `Kind::Custom`.
    pub method: String,
    pub content_type: String,
    pub body: String,
}

impl Default for Webhook {
    fn default() -> Self {
        Self {
            id: 0,
            kind: Kind::Discord,
            enabled: true,
            url: String::new(),
            method: "POST".into(),
            content_type: "application/json".into(),
            body: r#"{"title": "{title}", "message": "{message}"}"#.into(),
        }
    }
}

/// Values substituted into message templates as `{name}`.
pub type Vars = Vec<(&'static str, String)>;

pub fn render(template: &str, vars: &Vars, escape: impl Fn(&str) -> String) -> String {
    vars.iter()
        .fold(template.to_string(), |acc, (k, v)| acc.replace(&format!("{{{k}}}"), &escape(v)))
}

fn json_escape(s: &str) -> String {
    let quoted = serde_json::to_string(s).unwrap_or_default();
    quoted[1..quoted.len() - 1].to_string()
}

fn url_encode(s: &str) -> String {
    String::from(js_sys::encode_uri_component(s))
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn is_simple_content_type(ct: &str) -> bool {
    let ct = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    matches!(
        ct.as_str(),
        "text/plain" | "application/x-www-form-urlencoded" | "multipart/form-data"
    )
}

pub enum Delivery {
    /// The server answered with a 2xx we could read.
    Confirmed,
    /// Sent in `no-cors` mode: the request left, but the response is opaque.
    Sent,
}

fn js_err(e: JsValue) -> String {
    e.as_string()
        .or_else(|| js_sys::Reflect::get(&e, &"message".into()).ok().and_then(|m| m.as_string()))
        .unwrap_or_else(|| tr().err_unknown.into())
}

pub async fn send(hook: &Webhook, title: &str, message: &str, vars: &Vars) -> Result<Delivery, String> {
    let mut url = hook.url.trim().to_string();
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(tr().err_scheme.into());
    }

    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    let mut content_type = None;
    let mut no_cors = false;

    match hook.kind {
        Kind::Discord => {
            // Multipart with `payload_json` avoids the CORS preflight.
            let fd = web_sys::FormData::new().map_err(js_err)?;
            let payload = serde_json::json!({ "content": message }).to_string();
            fd.append_with_str("payload_json", &payload).map_err(js_err)?;
            init.set_body(&fd);
        }
        Kind::Slack => {
            // Slack sends no CORS headers; form-encoded `payload` is accepted.
            let payload = serde_json::json!({ "text": message }).to_string();
            init.set_body(&JsValue::from_str(&form(&[("payload", &payload)])));
            content_type = Some("application/x-www-form-urlencoded".to_string());
            no_cors = true;
        }
        Kind::Ntfy => {
            // Title as a query parameter instead of a header keeps the request simple.
            let sep = if url.contains('?') { '&' } else { '?' };
            url = format!("{url}{sep}title={}&tags=tomato", url_encode(title));
            init.set_body(&JsValue::from_str(message));
            content_type = Some("text/plain".to_string());
        }
        Kind::Gotify => {
            init.set_body(&JsValue::from_str(&form(&[
                ("title", title),
                ("message", message),
                ("priority", "5"),
            ])));
            content_type = Some("application/x-www-form-urlencoded".to_string());
            no_cors = true;
        }
        Kind::Custom => {
            let ct = hook.content_type.trim().to_string();
            let body = if ct.contains("json") {
                render(&hook.body, vars, json_escape)
            } else if ct.contains("x-www-form-urlencoded") {
                render(&hook.body, vars, url_encode)
            } else {
                render(&hook.body, vars, str::to_string)
            };
            let method = hook.method.trim().to_ascii_uppercase();
            init.set_method(if method.is_empty() { "POST" } else { &method });
            if method != "GET" && method != "HEAD" {
                init.set_body(&JsValue::from_str(&body));
            }
            no_cors = is_simple_content_type(&ct) && matches!(method.as_str(), "POST" | "GET" | "HEAD");
            if !ct.is_empty() {
                content_type = Some(ct);
            }
        }
    }

    if let Some(ct) = content_type {
        let headers = js_sys::Object::new();
        js_sys::Reflect::set(&headers, &"Content-Type".into(), &ct.into()).map_err(js_err)?;
        init.set_headers(&headers);
    }
    if no_cors {
        init.set_mode(web_sys::RequestMode::NoCors);
    }

    let resp = JsFuture::from(web_sys::window().unwrap().fetch_with_str_and_init(&url, &init))
        .await
        .map_err(|e| {
            let msg = js_err(e);
            if hook.kind == Kind::Custom && !no_cors {
                format!("{msg} ({})", tr().err_cors)
            } else {
                msg
            }
        })?;
    let resp: web_sys::Response = resp.dyn_into().map_err(js_err)?;

    if no_cors {
        Ok(Delivery::Sent)
    } else if resp.ok() {
        Ok(Delivery::Confirmed)
    } else {
        Err(format!("HTTP {}", resp.status()))
    }
}
