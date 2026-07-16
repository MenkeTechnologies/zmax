//! OpenAI backend (Chat Completions API).

use super::{read_response, Message, Provider};

const DEFAULT_MODEL: &str = "gpt-4o";
const API_URL: &str = "https://api.openai.com/v1/chat/completions";

pub struct OpenAi {
    key: String,
    model: String,
}

impl OpenAi {
    /// Build from the environment; errors if `OPENAI_API_KEY` is unset.
    pub fn from_env(model: Option<String>) -> Result<Self, String> {
        let key = std::env::var("OPENAI_API_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "OPENAI_API_KEY is not set".to_string())?;
        Ok(Self {
            key,
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        })
    }

    /// Build the request body. Unlike Anthropic, the system prompt is just the first message.
    pub(crate) fn body(
        model: &str,
        system: Option<&str>,
        messages: &[Message],
    ) -> serde_json::Value {
        let mut msgs: Vec<serde_json::Value> = Vec::new();
        if let Some(sys) = system {
            msgs.push(serde_json::json!({ "role": "system", "content": sys }));
        }
        for m in messages {
            msgs.push(serde_json::json!({ "role": m.role.as_str(), "content": m.content }));
        }
        serde_json::json!({ "model": model, "messages": msgs })
    }

    /// Extract `choices[0].message.content` from a Chat Completions response.
    pub(crate) fn parse(resp: &str) -> Result<String, String> {
        let v: serde_json::Value =
            serde_json::from_str(resp).map_err(|e| format!("openai: parse: {e}"))?;
        if let Some(msg) = v["error"]["message"].as_str() {
            return Err(format!("openai: {msg}"));
        }
        Ok(v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }
}

impl Provider for OpenAi {
    fn name(&self) -> &'static str {
        "openai"
    }
    fn model(&self) -> &str {
        &self.model
    }
    fn chat(&self, system: Option<&str>, messages: &[Message]) -> Result<String, String> {
        let body = Self::body(&self.model, system, messages);
        let resp = read_response(
            ureq::post(API_URL)
                .set("Authorization", &format!("Bearer {}", self.key))
                .set("content-type", "application/json")
                .send_json(body),
            "openai",
        )?;
        Self::parse(&resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::Role;

    #[test]
    fn body_prepends_system_message() {
        let msgs = vec![Message::user("hello")];
        let b = OpenAi::body("m", Some("be brief"), &msgs);
        let arr = b["messages"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["role"], "system");
        assert_eq!(arr[1]["role"], "user");
        let _ = Role::User;
    }

    #[test]
    fn parse_extracts_content() {
        let r = r#"{"choices":[{"message":{"role":"assistant","content":"hi there"}}]}"#;
        assert_eq!(OpenAi::parse(r).unwrap(), "hi there");
    }

    #[test]
    fn parse_surfaces_error() {
        let r = r#"{"error":{"message":"bad key"}}"#;
        assert!(OpenAi::parse(r).unwrap_err().contains("bad key"));
    }
}
