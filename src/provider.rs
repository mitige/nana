//! multi-provider: one client, every shape.
//!
//! the endpoint is picked from the model name (a `claude-*` model goes to
//! anthropic, `gemini-*` to google, `llama*`/`qwen*` to a local ollama when
//! there is one, everything else to an openai-compatible `/v1`), and any of
//! those can be overridden in settings.json. keys come from the environment,
//! then from `~/.dsh/.credentials.yaml`, then from the user's settings.
//!
//! three wire shapes are implemented, which covers every provider listed:
//! openai-compatible (openai, dashscope, openrouter, ollama, agentic press,
//! and anything else you point it at), anthropic, and gemini.

use crate::settings::Settings;
use serde_json::{json, Value};
use std::path::Path;

/// post json and read json back, turning every failure into a sentence.
macro_rules! send_json {
    ($req:expr, $body:expr) => {{
        let payload = serde_json::to_string($body)
            .map_err(|e| format!("this request cannot be serialised: {e}"))?;
        match $req.send(payload) {
            Ok(mut resp) => resp
                .body_mut()
                .read_json::<Value>()
                .map_err(|e| format!("unreadable answer: {e}"))?,
            Err(ureq::Error::StatusCode(code)) => {
                return Err(format!("the provider answered http {code}"))
            }
            Err(e) => return Err(format!("network error: {e}")),
        }
    }};
}

/// how a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// POST {base}/chat/completions, bearer token — the lingua franca
    OpenAi,
    /// POST {base}/v1/messages, x-api-key + anthropic-version
    Anthropic,
    /// POST {base}/v1beta/models/{model}:generateContent, key in the query
    Gemini,
}

#[derive(Debug, Clone, Copy)]
pub struct Provider {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    pub base_url: &'static str,
    /// environment variable that holds the key, if the provider wants one
    pub key_env: Option<&'static str>,
    /// fragments that identify this provider from a model name
    pub model_hints: &'static [&'static str],
    /// can run with no key at all (a local daemon)
    pub keyless: bool,
}

/// every provider nana knows out of the box.
pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "anthropic",
        label: "anthropic",
        kind: Kind::Anthropic,
        base_url: "https://api.anthropic.com",
        key_env: Some("ANTHROPIC_API_KEY"),
        model_hints: &["claude", "sonnet", "opus", "haiku"],
        keyless: false,
    },
    Provider {
        id: "openai",
        label: "openai",
        kind: Kind::OpenAi,
        base_url: "https://api.openai.com/v1",
        key_env: Some("OPENAI_API_KEY"),
        model_hints: &["gpt-", "o1", "o3", "o4", "chatgpt"],
        keyless: false,
    },
    Provider {
        id: "gemini",
        label: "gemini",
        kind: Kind::Gemini,
        base_url: "https://generativelanguage.googleapis.com",
        key_env: Some("GEMINI_API_KEY"),
        model_hints: &["gemini"],
        keyless: false,
    },
    Provider {
        id: "dashscope",
        label: "alibaba dashscope",
        kind: Kind::OpenAi,
        base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        key_env: Some("DASHSCOPE_API_KEY"),
        model_hints: &["qwen", "kimi", "deepseek", "glm", "minimax", "abab"],
        keyless: false,
    },
    Provider {
        id: "openrouter",
        label: "openrouter",
        kind: Kind::OpenAi,
        base_url: "https://openrouter.ai/api/v1",
        key_env: Some("OPENROUTER_API_KEY"),
        model_hints: &["openrouter/"],
        keyless: false,
    },
    Provider {
        id: "agentic-press",
        label: "agentic press",
        kind: Kind::OpenAi,
        base_url: "https://api.agentic.press/v1",
        key_env: Some("AGENTIC_PRESS_API_KEY"),
        model_hints: &["agentic/"],
        keyless: false,
    },
    Provider {
        id: "ollama",
        label: "ollama (local)",
        kind: Kind::OpenAi,
        base_url: "http://127.0.0.1:11434/v1",
        key_env: None,
        model_hints: &["llama", "mistral", "phi", "gemma", "codellama", "ollama/"],
        keyless: true,
    },
    Provider {
        id: "openai-compatible",
        label: "openai-compatible /v1",
        kind: Kind::OpenAi,
        base_url: "https://api.openai.com/v1",
        key_env: Some("OPENAI_API_KEY"),
        model_hints: &[],
        keyless: false,
    },
];

impl Provider {
    /// how this provider speaks, as a word for the ui.
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            Kind::OpenAi => "openai",
            Kind::Anthropic => "anthropic",
            Kind::Gemini => "gemini",
        }
    }
}

/// is a key of that name present in the dsh credentials file? (never prints it)
pub fn dsh_has(name: &str) -> bool {
    key_from_dsh(name).is_some()
}

pub fn by_id(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// which provider a model name implies. the last entry is the fallback, so
/// this never fails: an unknown model goes to the openai-compatible shape.
pub fn detect(model: &str) -> &'static Provider {
    let m = model.to_ascii_lowercase();
    // a slash prefix wins: "openrouter/anthropic/claude-3" means openrouter
    if let Some((head, _)) = m.split_once('/') {
        if let Some(p) = by_id(head) {
            return p;
        }
    }
    for p in PROVIDERS {
        if p.model_hints.iter().any(|h| m.contains(h)) {
            return p;
        }
    }
    by_id("openai-compatible").unwrap_or(&PROVIDERS[0])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    fn wire(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// one turn of the conversation.
#[derive(Debug, Clone)]
pub struct Msg {
    pub role: Role,
    pub content: String,
    /// set on an assistant turn that asked for tools
    pub tool_calls: Vec<ToolCall>,
    /// set on the tool result that answers a call
    pub tool_call_id: Option<String>,
}

impl Msg {
    pub fn user(text: impl Into<String>) -> Msg {
        Msg {
            role: Role::User,
            content: text.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
    pub fn tool(id: &str, text: impl Into<String>) -> Msg {
        Msg {
            role: Role::Tool,
            content: text.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(id.to_string()),
        }
    }
}

/// a tool the model asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

/// what the model answered.
#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}

/// a tool declaration, in the shape every provider accepts after translation.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// json schema for the arguments
    pub parameters: Value,
}

pub struct Client {
    pub provider: &'static Provider,
    pub base_url: String,
    pub model: String,
    api_key: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    agent: ureq::Agent,
}

impl Client {
    /// build a client from settings, resolving the key from the environment
    /// first, then from the dsh credentials file.
    pub fn resolve(settings: &Settings, root: &Path) -> Result<Client, String> {
        let model = settings
            .model
            .clone()
            .or_else(|| std::env::var("NANA_MODEL").ok())
            .unwrap_or_else(|| "kimi-k3".to_string());
        let provider = settings
            .provider
            .as_deref()
            .and_then(by_id)
            .unwrap_or_else(|| detect(&model));
        let base_url = settings
            .base_url
            .clone()
            .or_else(|| std::env::var("NANA_BASE_URL").ok())
            .unwrap_or_else(|| provider.base_url.to_string());
        let key_env = settings
            .api_key_env
            .clone()
            .or_else(|| provider.key_env.map(str::to_string));
        // the key is looked up for its own provider, never borrowed from
        // another one: a dashscope key has no business in an anthropic header
        let mut api_key = key_env
            .as_deref()
            .and_then(key_from_env)
            .or_else(|| key_env.as_deref().and_then(|e| key_from_dsh(e)));
        if api_key.is_none() && provider.id == "dashscope" {
            api_key = key_from_dsh("OPP_API_KEY").or_else(|| key_from_env("OPP_API_KEY"));
        }
        // a local endpoint (a mock, or your own gateway) does not want a key
        let local = base_url.contains("127.0.0.1") || base_url.contains("localhost");
        if api_key.is_none() && !provider.keyless && !local {
            return Err(format!(
                "no api key for {} — set {} or add it to ~/.dsh/.credentials.yaml",
                provider.id,
                key_env.unwrap_or_else(|| "NANA_API_KEY".into())
            ));
        }
        let _ = root;
        let timeout = std::time::Duration::from_secs(300);
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .build()
            .into();
        Ok(Client {
            provider,
            base_url,
            model,
            api_key,
            temperature: settings.temperature,
            max_tokens: settings.max_tokens,
            agent,
        })
    }

    /// attach a key (used by the tests and by the tui when the user types one).
    pub fn with_api_key(mut self, key: impl Into<String>) -> Client {
        self.api_key = Some(key.into());
        self
    }

    pub fn has_key(&self) -> bool {
        self.api_key.is_some()
    }

    /// a client for a local endpoint, no key: used by the tests and by anyone
    /// pointing nana at a mock.
    pub fn local(base_url: &str, model: &str) -> Client {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build()
            .into();
        Client {
            provider: by_id("openai-compatible").unwrap(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            api_key: None,
            temperature: None,
            max_tokens: None,
            agent,
        }
    }

    /// one model call, with optional tool declarations.
    pub fn chat(
        &self,
        system: &str,
        messages: &[Msg],
        tools: &[ToolSpec],
    ) -> Result<Reply, String> {
        match self.provider.kind {
            Kind::OpenAi => self.chat_openai(system, messages, tools),
            Kind::Anthropic => self.chat_anthropic(system, messages, tools),
            Kind::Gemini => self.chat_gemini(system, messages, tools),
        }
    }

    // ------------------------------------------------------------ openai shape
    fn chat_openai(
        &self,
        system: &str,
        messages: &[Msg],
        tools: &[ToolSpec],
    ) -> Result<Reply, String> {
        let mut wire: Vec<Value> = Vec::new();
        if !system.is_empty() {
            wire.push(json!({"role": "system", "content": system}));
        }
        for m in messages {
            match m.role {
                Role::Assistant if !m.tool_calls.is_empty() => {
                    let calls: Vec<Value> = m
                        .tool_calls
                        .iter()
                        .map(|c| {
                            json!({
                                "id": c.id,
                                "type": "function",
                                "function": {"name": c.name, "arguments": c.args.to_string()}
                            })
                        })
                        .collect();
                    wire.push(json!({
                        "role": "assistant",
                        "content": m.content,
                        "tool_calls": calls
                    }));
                }
                Role::Tool => wire.push(json!({
                    "role": "tool",
                    "tool_call_id": m.tool_call_id.clone().unwrap_or_default(),
                    "content": m.content
                })),
                other => wire.push(json!({"role": other.wire(), "content": m.content})),
            }
        }
        let mut body = json!({"model": self.model, "messages": wire});
        if let Some(t) = self.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(m) = self.max_tokens {
            body["max_tokens"] = json!(m);
        }
        if !tools.is_empty() {
            body["tools"] = json!(tools
                .iter()
                .map(|t| json!({
                    "type": "function",
                    "function": {"name": t.name, "description": t.description, "parameters": t.parameters}
                }))
                .collect::<Vec<_>>());
            body["tool_choice"] = json!("auto");
        }
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut req = self
            .agent
            .post(&url)
            .header("Content-Type", "application/json");
        if let Some(k) = &self.api_key {
            req = req.header("Authorization", &format!("Bearer {k}"));
        }
        let value = send_json!(req, &body);
        let message = value
            .pointer("/choices/0/message")
            .cloned()
            .unwrap_or(Value::Null);
        let text = message
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string();
        let mut calls = Vec::new();
        if let Some(arr) = message.get("tool_calls").and_then(|c| c.as_array()) {
            for (i, c) in arr.iter().enumerate() {
                let name = c
                    .pointer("/function/name")
                    .and_then(|n| n.as_str())
                    .unwrap_or_default()
                    .to_string();
                let raw = c
                    .pointer("/function/arguments")
                    .and_then(|a| a.as_str())
                    .unwrap_or("{}");
                calls.push(ToolCall {
                    id: c
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("call_{i}")),
                    name,
                    args: serde_json::from_str(raw).unwrap_or_else(|_| json!({})),
                });
            }
        }
        Ok(Reply {
            text,
            tool_calls: calls,
            prompt_tokens: value
                .pointer("/usage/prompt_tokens")
                .and_then(|v| v.as_u64()),
            completion_tokens: value
                .pointer("/usage/completion_tokens")
                .and_then(|v| v.as_u64()),
        })
    }

    // -------------------------------------------------------- anthropic shape
    fn chat_anthropic(
        &self,
        system: &str,
        messages: &[Msg],
        tools: &[ToolSpec],
    ) -> Result<Reply, String> {
        let mut wire: Vec<Value> = Vec::new();
        for m in messages {
            match m.role {
                Role::System => {}
                Role::Assistant if !m.tool_calls.is_empty() => {
                    let mut blocks: Vec<Value> = Vec::new();
                    if !m.content.is_empty() {
                        blocks.push(json!({"type": "text", "text": m.content}));
                    }
                    for c in &m.tool_calls {
                        blocks.push(json!({
                            "type": "tool_use", "id": c.id, "name": c.name, "input": c.args
                        }));
                    }
                    wire.push(json!({"role": "assistant", "content": blocks}));
                }
                Role::Tool => wire.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                        "content": m.content
                    }]
                })),
                Role::User => wire.push(json!({"role": "user", "content": m.content})),
                Role::Assistant => wire.push(json!({"role": "assistant", "content": m.content})),
            }
        }
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens.unwrap_or(8192),
            "messages": wire
        });
        if !system.is_empty() {
            body["system"] = json!(system);
        }
        if let Some(t) = self.temperature {
            body["temperature"] = json!(t);
        }
        if !tools.is_empty() {
            body["tools"] = json!(tools
                .iter()
                .map(|t| json!({
                    "name": t.name, "description": t.description, "input_schema": t.parameters
                }))
                .collect::<Vec<_>>());
        }
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let mut req = self
            .agent
            .post(&url)
            .header("Content-Type", "application/json")
            .header("anthropic-version", "2023-06-01");
        if let Some(k) = &self.api_key {
            req = req.header("x-api-key", k);
        }
        let value = send_json!(req, &body);
        let mut text = String::new();
        let mut calls = Vec::new();
        if let Some(blocks) = value.get("content").and_then(|c| c.as_array()) {
            for (i, b) in blocks.iter().enumerate() {
                match b.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                            text.push_str(t);
                        }
                    }
                    Some("tool_use") => calls.push(ToolCall {
                        id: b
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("toolu_{i}")),
                        name: b
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        args: b.get("input").cloned().unwrap_or_else(|| json!({})),
                    }),
                    _ => {}
                }
            }
        }
        Ok(Reply {
            text,
            tool_calls: calls,
            prompt_tokens: value
                .pointer("/usage/input_tokens")
                .and_then(|v| v.as_u64()),
            completion_tokens: value
                .pointer("/usage/output_tokens")
                .and_then(|v| v.as_u64()),
        })
    }

    // ----------------------------------------------------------- gemini shape
    fn chat_gemini(
        &self,
        system: &str,
        messages: &[Msg],
        tools: &[ToolSpec],
    ) -> Result<Reply, String> {
        let mut contents: Vec<Value> = Vec::new();
        for m in messages {
            match m.role {
                Role::System => {}
                Role::Tool => contents.push(json!({
                    "role": "user",
                    "parts": [{"functionResponse": {
                        "name": m.tool_call_id.clone().unwrap_or_default(),
                        "response": {"result": m.content}
                    }}]
                })),
                Role::Assistant if !m.tool_calls.is_empty() => {
                    let mut parts: Vec<Value> = Vec::new();
                    if !m.content.is_empty() {
                        parts.push(json!({"text": m.content}));
                    }
                    for c in &m.tool_calls {
                        parts.push(json!({"functionCall": {"name": c.name, "args": c.args}}));
                    }
                    contents.push(json!({"role": "model", "parts": parts}));
                }
                Role::Assistant => {
                    contents.push(json!({"role": "model", "parts": [{"text": m.content}]}))
                }
                Role::User => {
                    contents.push(json!({"role": "user", "parts": [{"text": m.content}]}))
                }
            }
        }
        let mut body = json!({"contents": contents});
        if !system.is_empty() {
            body["systemInstruction"] = json!({"parts": [{"text": system}]});
        }
        if let Some(t) = self.temperature {
            body["generationConfig"] = json!({"temperature": t});
        }
        if !tools.is_empty() {
            body["tools"] = json!([{
                "functionDeclarations": tools
                    .iter()
                    .map(|t| json!({
                        "name": t.name, "description": t.description, "parameters": t.parameters
                    }))
                    .collect::<Vec<_>>()
            }]);
        }
        let url = format!(
            "{}/v1beta/models/{}:generateContent",
            self.base_url.trim_end_matches('/'),
            self.model
        );
        let mut req = self
            .agent
            .post(&url)
            .header("Content-Type", "application/json");
        if let Some(k) = &self.api_key {
            req = req.header("x-goog-api-key", k);
        }
        let value = send_json!(req, &body);
        let mut text = String::new();
        let mut calls = Vec::new();
        if let Some(parts) = value
            .pointer("/candidates/0/content/parts")
            .and_then(|p| p.as_array())
        {
            for (i, p) in parts.iter().enumerate() {
                if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
                    text.push_str(t);
                }
                if let Some(fc) = p.get("functionCall") {
                    calls.push(ToolCall {
                        id: format!("gemini_{i}"),
                        name: fc
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        args: fc.get("args").cloned().unwrap_or_else(|| json!({})),
                    });
                }
            }
        }
        Ok(Reply {
            text,
            tool_calls: calls,
            prompt_tokens: value
                .pointer("/usageMetadata/promptTokenCount")
                .and_then(|v| v.as_u64()),
            completion_tokens: value
                .pointer("/usageMetadata/candidatesTokenCount")
                .and_then(|v| v.as_u64()),
        })
    }
}

/// the key is never printed, not even by a `{:?}` of the client.
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("provider", &self.provider.id)
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .finish()
    }
}

fn key_from_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|k| !k.trim().is_empty())
}

/// read a key from `~/.dsh/.credentials.yaml` without ever printing it.
fn key_from_dsh(name: &str) -> Option<String> {
    let path = crate::config::home_dir()
        .join(".dsh")
        .join(".credentials.yaml");
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix(&format!("{name}:")) {
            let v = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {

    /// the json body of a captured request.
    fn body_of(req: &str) -> Value {
        let body = req.split("\r\n\r\n").nth(1).unwrap_or_default();
        serde_json::from_str(body).unwrap_or_else(|e| panic!("body is not json ({e}): {body}"))
    }

    /// read a whole http request: the headers, then exactly as many body bytes
    /// as content-length promised. deterministic, no waiting on timeouts.
    fn read_request(sock: &mut std::net::TcpStream) -> String {
        use std::io::Read;
        sock.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .ok();
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 8192];
        let mut head_len: Option<usize> = None;
        let mut want: usize = 0;
        loop {
            if let Some(h) = head_len {
                if buf.len() >= h + want {
                    break;
                }
            }
            match sock.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(_) => break,
            }
            if head_len.is_none() {
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    head_len = Some(i + 4);
                    let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                    want = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                }
            }
        }
        String::from_utf8_lossy(&buf).to_string()
    }
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn models_are_routed_to_their_provider() {
        assert_eq!(detect("claude-sonnet-4").id, "anthropic");
        assert_eq!(detect("gemini-2.5-pro").id, "gemini");
        assert_eq!(detect("qwen-max").id, "dashscope");
        assert_eq!(detect("kimi-k3").id, "dashscope");
        assert_eq!(detect("gpt-4o").id, "openai");
        assert_eq!(detect("llama3.1:8b").id, "ollama");
        assert_eq!(detect("openrouter/anthropic/claude-3").id, "openrouter");
        // anything unknown still lands on the openai-compatible shape
        assert_eq!(detect("some-new-model").id, "openai-compatible");
    }

    /// a real http server, so the client is exercised end to end: request
    /// shape in, answer parsed out.
    fn mock(reply: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let req = read_request(&mut sock);
            let body = reply.to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            sock.write_all(resp.as_bytes()).unwrap();
            req
        });
        (format!("http://{addr}"), handle)
    }

    #[test]
    fn an_openai_answer_becomes_text_and_tool_calls() {
        let (url, server) = mock(
            r#"{"choices":[{"message":{"content":"bonjour","tool_calls":[
                 {"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"a.txt\"}"}}
               ]}}],"usage":{"prompt_tokens":11,"completion_tokens":2}}"#,
        );
        let c = Client::local(&url, "mock-model");
        let reply = c.chat("system", &[Msg::user("hi")], &[]).unwrap();
        assert_eq!(reply.text, "bonjour");
        assert_eq!(reply.tool_calls.len(), 1);
        assert_eq!(reply.tool_calls[0].name, "read_file");
        assert_eq!(reply.tool_calls[0].args["path"], "a.txt");
        assert_eq!(reply.prompt_tokens, Some(11));
        // and the request we sent carried the system message and the model
        let req = server.join().unwrap();
        let body = body_of(&req);
        assert_eq!(body["model"], "mock-model");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
    }

    #[test]
    fn tool_declarations_are_sent_in_the_openai_shape() {
        let (url, server) = mock(r#"{"choices":[{"message":{"content":"ok"}}]}"#);
        let c = Client::local(&url, "m");
        let tools = vec![ToolSpec {
            name: "read_file".into(),
            description: "read".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}}}),
        }];
        c.chat("", &[Msg::user("hi")], &tools).unwrap();
        let req = server.join().unwrap();
        let body = body_of(&req);
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "read_file");
        assert_eq!(body["tool_choice"], "auto");
    }

    #[test]
    fn an_http_error_is_reported_not_panicked() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let _ = read_request(&mut sock);
            let _ = sock.write_all(
                b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}",
            );
        });
        let c = Client::local(&format!("http://{addr}"), "m");
        let err = c.chat("", &[Msg::user("hi")], &[]).unwrap_err();
        assert!(err.contains("401"), "{err}");
    }

    #[test]
    fn anthropic_answers_are_parsed() {
        let (url, server) = mock(
            r#"{"content":[{"type":"text","text":"salut"},
                {"type":"tool_use","id":"toolu_1","name":"grep","input":{"pattern":"x"}}],
                "usage":{"input_tokens":3,"output_tokens":4}}"#,
        );
        let mut c = Client::local(&url, "claude-sonnet-4").with_api_key("k");
        c.provider = by_id("anthropic").unwrap();
        let reply = c.chat("sys", &[Msg::user("hi")], &[]).unwrap();
        assert_eq!(reply.text, "salut");
        assert_eq!(reply.tool_calls[0].name, "grep");
        assert_eq!(reply.completion_tokens, Some(4));
        let req = server.join().unwrap();
        assert!(req.contains("x-api-key: k"), "anthropic header: {req}");
        assert!(req.contains("/v1/messages"), "{req}");
        let body = body_of(&req);
        assert_eq!(body["system"], "sys");
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn gemini_answers_are_parsed() {
        let (url, server) = mock(
            r#"{"candidates":[{"content":{"parts":[{"text":"hey"},
                {"functionCall":{"name":"read_file","args":{"path":"b"}}}]}}],
                "usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":6}}"#,
        );
        let mut c = Client::local(&url, "gemini-2.5-flash");
        c.provider = by_id("gemini").unwrap();
        let reply = c.chat("sys", &[Msg::user("hi")], &[]).unwrap();
        assert_eq!(reply.text, "hey");
        assert_eq!(reply.tool_calls[0].args["path"], "b");
        assert_eq!(reply.prompt_tokens, Some(5));
        let req = server.join().unwrap();
        assert!(req.contains(":generateContent"), "{req}");
        let body = body_of(&req);
        assert_eq!(body["contents"][0]["parts"][0]["text"], "hi");
    }

    #[test]
    fn a_missing_key_is_explained() {
        let d = std::env::temp_dir().join(format!("nana-prov-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let mut s = Settings::default();
        s.model = Some("claude-sonnet-4".into());
        s.api_key_env = Some("NANA_TEST_KEY_THAT_DOES_NOT_EXIST".into());
        std::env::remove_var("NANA_TEST_KEY_THAT_DOES_NOT_EXIST");
        let err = Client::resolve(&s, &d).unwrap_err();
        assert!(err.contains("no api key"), "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
