//! Anthropic Claude backend (Messages API).

use super::{read_response, Message, Provider, Role};

const DEFAULT_MODEL: &str = "claude-3-5-sonnet-latest";
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const MAX_TOKENS: u32 = 4096;

pub struct Anthropic {
    key: String,
    model: String,
}

impl Anthropic {
    /// Build from the environment; errors if `ANTHROPIC_API_KEY` is unset.
    pub fn from_env(model: Option<String>) -> Result<Self, String> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "ANTHROPIC_API_KEY is not set".to_string())?;
        Ok(Self {
            key,
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        })
    }

    /// Build the JSON request body (separated out for testing without a network call).
    pub(crate) fn body(model: &str, system: Option<&str>, messages: &[Message]) -> serde_json::Value {
        let msgs: Vec<serde_json::Value> = messages
            .iter()
            // Anthropic's `messages` only takes user/assistant; the system prompt is top-level.
            .filter(|m| m.role != Role::System)
            .map(|m| serde_json::json!({ "role": m.role.as_str(), "content": m.content }))
            .collect();
        let mut body = serde_json::json!({
            "model": model,
            "max_tokens": MAX_TOKENS,
            "messages": msgs,
        });
        if let Some(sys) = system {
            body["system"] = serde_json::Value::String(sys.to_string());
        }
        body
    }

    /// Extract the concatenated text from a Messages API response body.
    pub(crate) fn parse(resp: &str) -> Result<String, String> {
        let v: serde_json::Value =
            serde_json::from_str(resp).map_err(|e| format!("anthropic: parse: {e}"))?;
        if let Some(msg) = v["error"]["message"].as_str() {
            return Err(format!("anthropic: {msg}"));
        }
        let text = v["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        Ok(text)
    }
}

impl Provider for Anthropic {
    fn name(&self) -> &'static str {
        "anthropic"
    }
    fn model(&self) -> &str {
        &self.model
    }
    fn chat(&self, system: Option<&str>, messages: &[Message]) -> Result<String, String> {
        let body = Self::body(&self.model, system, messages);
        let resp = read_response(
            ureq::post(API_URL)
                .set("x-api-key", &self.key)
                .set("anthropic-version", "2023-06-01")
                .set("content-type", "application/json")
                .send_json(body),
            "anthropic",
        )?;
        Self::parse(&resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_puts_system_top_level_and_filters_system_messages() {
        let msgs = vec![
            Message {
                role: Role::System,
                content: "ignored".into(),
            },
            Message::user("hello"),
        ];
        let b = Anthropic::body("m", Some("be brief"), &msgs);
        assert_eq!(b["system"], "be brief");
        assert_eq!(b["messages"].as_array().unwrap().len(), 1);
        assert_eq!(b["messages"][0]["role"], "user");
        assert_eq!(b["messages"][0]["content"], "hello");
    }

    #[test]
    fn parse_extracts_text_blocks() {
        let r = r#"{"content":[{"type":"text","text":"hi "},{"type":"text","text":"there"}]}"#;
        assert_eq!(Anthropic::parse(r).unwrap(), "hi there");
    }

    #[test]
    fn parse_surfaces_error() {
        let r = r#"{"type":"error","error":{"type":"x","message":"bad key"}}"#;
        assert!(Anthropic::parse(r).unwrap_err().contains("bad key"));
    }
}
