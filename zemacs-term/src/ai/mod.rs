//! AI provider abstraction for zemacs' AI-agent integration.
//!
//! Pluggable LLM backends (Anthropic, OpenAI) behind one [`Provider`] trait. The chat panel,
//! inline-edit, and (later) the autonomous agent all talk to a `Provider` rather than a specific
//! vendor. Phase 1 is non-streaming chat; streaming and tool-use are layered on later.
//!
//! Configuration is read from the environment (and, later, `config.toml`):
//! - `ZEMACS_AI_PROVIDER` — `anthropic` (default) or `openai`
//! - `ZEMACS_AI_MODEL` — overrides the provider's default model
//! - `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` — the API key for the chosen provider

pub mod agent;
pub mod anthropic;
pub mod openai;

/// A chat role. zemacs keeps `System` separate from the message list (Anthropic wants it
/// top-level), but the enum models all three for OpenAI-style backends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

/// One chat message.
#[derive(Clone, Debug)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// A tool the agent can call (name + description + JSON-Schema for its input).
#[derive(Clone, Debug)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// A model request to invoke a tool.
#[derive(Clone, Debug)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// One content block within an agent turn.
#[derive(Clone, Debug)]
pub enum Content {
    Text(String),
    ToolUse(ToolUse),
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

/// One turn in an agent conversation (richer than [`Message`]: carries tool blocks).
#[derive(Clone, Debug)]
pub struct Turn {
    pub role: Role,
    pub content: Vec<Content>,
}

impl Turn {
    pub fn user_text(s: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![Content::Text(s.into())],
        }
    }
}

/// The assistant's reply to an agent turn: any text it emitted, the tool calls it wants run, and
/// the stop reason (`"tool_use"` means it expects tool results and the loop should continue).
#[derive(Clone, Debug, Default)]
pub struct AssistantReply {
    pub text: String,
    pub tool_uses: Vec<ToolUse>,
    pub stop_reason: String,
}

/// A chat backend. `chat` blocks on the network — call it from `tokio::task::spawn_blocking`,
/// never on the UI thread. `system` is the system prompt; `messages` are the user/assistant turns.
pub trait Provider: Send + Sync {
    fn name(&self) -> &'static str;
    fn model(&self) -> &str;
    fn chat(&self, system: Option<&str>, messages: &[Message]) -> Result<String, String>;

    /// Whether this backend implements agent tool-use ([`Provider::agent_turn`]).
    fn supports_tools(&self) -> bool {
        false
    }

    /// One agent step: send the running conversation + available tools, get back the assistant's
    /// text and any tool calls. Default: unsupported (overridden by tool-capable backends).
    fn agent_turn(
        &self,
        _system: Option<&str>,
        _turns: &[Turn],
        _tools: &[Tool],
    ) -> Result<AssistantReply, String> {
        Err(format!(
            "{} does not support agent tool-use yet (set ZEMACS_AI_PROVIDER=anthropic)",
            self.name()
        ))
    }
}

/// Resolve `ZEMACS_AI_PROVIDER` / `ZEMACS_AI_MODEL` from the environment.
fn provider_and_model() -> (String, Option<String>) {
    let provider = std::env::var("ZEMACS_AI_PROVIDER")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "anthropic".to_string());
    let model = std::env::var("ZEMACS_AI_MODEL").ok().filter(|s| !s.is_empty());
    (provider, model)
}

/// Build the configured provider, or a human-readable error describing what's missing
/// (e.g. an unset API key or an unknown provider name).
pub fn provider() -> Result<Box<dyn Provider>, String> {
    let (provider, model) = provider_and_model();
    match provider.as_str() {
        "anthropic" => anthropic::Anthropic::from_env(model).map(|p| Box::new(p) as Box<dyn Provider>),
        "openai" => openai::OpenAi::from_env(model).map(|p| Box::new(p) as Box<dyn Provider>),
        other => Err(format!(
            "unknown ZEMACS_AI_PROVIDER '{other}' (use 'anthropic' or 'openai')"
        )),
    }
}

/// Read an HTTP response body, turning a non-2xx status into a descriptive error that includes
/// the response body (which usually carries the provider's error message). Shared by backends.
pub(crate) fn read_response(
    result: Result<ureq::Response, ureq::Error>,
    provider: &str,
) -> Result<String, String> {
    match result {
        Ok(resp) => resp
            .into_string()
            .map_err(|e| format!("{provider}: reading response: {e}")),
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            Err(format!("{provider} HTTP {code}: {}", body.trim()))
        }
        Err(e) => Err(format!("{provider}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_strings() {
        assert_eq!(Role::User.as_str(), "user");
        assert_eq!(Role::Assistant.as_str(), "assistant");
        assert_eq!(Role::System.as_str(), "system");
    }

    #[test]
    fn message_helpers() {
        let m = Message::user("hi");
        assert_eq!(m.role, Role::User);
        assert_eq!(m.content, "hi");
    }

    #[test]
    fn unknown_provider_errors() {
        std::env::set_var("ZEMACS_AI_PROVIDER", "nope");
        let e = provider().err().expect("should error on unknown provider");
        assert!(e.contains("unknown"));
        std::env::remove_var("ZEMACS_AI_PROVIDER");
    }
}
