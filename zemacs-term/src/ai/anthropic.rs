//! Anthropic Claude backend (Messages API).

use super::{read_response, AssistantReply, Content, Message, Provider, Role, Tool, ToolUse, Turn};

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
    pub(crate) fn body(
        model: &str,
        system: Option<&str>,
        messages: &[Message],
    ) -> serde_json::Value {
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

    /// Serialize agent turns into Anthropic's `messages` array (content blocks).
    pub(crate) fn turns_json(turns: &[Turn]) -> Vec<serde_json::Value> {
        turns
            .iter()
            .map(|t| {
                let blocks: Vec<serde_json::Value> = t
                    .content
                    .iter()
                    .map(|c| match c {
                        Content::Text(s) => serde_json::json!({"type":"text","text":s}),
                        Content::ToolUse(tu) => serde_json::json!({
                            "type":"tool_use","id":tu.id,"name":tu.name,"input":tu.input
                        }),
                        Content::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => serde_json::json!({
                            "type":"tool_result","tool_use_id":tool_use_id,
                            "content":content,"is_error":is_error
                        }),
                    })
                    .collect();
                serde_json::json!({ "role": t.role.as_str(), "content": blocks })
            })
            .collect()
    }

    /// Parse an agent response: text blocks + tool_use blocks + stop_reason.
    pub(crate) fn parse_reply(resp: &str) -> Result<AssistantReply, String> {
        let v: serde_json::Value =
            serde_json::from_str(resp).map_err(|e| format!("anthropic: parse: {e}"))?;
        if let Some(msg) = v["error"]["message"].as_str() {
            return Err(format!("anthropic: {msg}"));
        }
        let mut reply = AssistantReply {
            stop_reason: v["stop_reason"].as_str().unwrap_or("").to_string(),
            ..Default::default()
        };
        if let Some(blocks) = v["content"].as_array() {
            for b in blocks {
                match b["type"].as_str() {
                    Some("text") => reply.text.push_str(b["text"].as_str().unwrap_or("")),
                    Some("tool_use") => reply.tool_uses.push(ToolUse {
                        id: b["id"].as_str().unwrap_or("").to_string(),
                        name: b["name"].as_str().unwrap_or("").to_string(),
                        input: b["input"].clone(),
                    }),
                    _ => {}
                }
            }
        }
        Ok(reply)
    }

    /// Parse one SSE `data:` payload line, returning the text delta if it is a `content_block_delta`
    /// of type `text_delta`. (Other events — message_start, ping, etc. — return None.)
    pub(crate) fn sse_delta(data: &str) -> Option<String> {
        let v: serde_json::Value = serde_json::from_str(data).ok()?;
        if v["type"].as_str() == Some("content_block_delta")
            && v["delta"]["type"].as_str() == Some("text_delta")
        {
            return v["delta"]["text"].as_str().map(|s| s.to_string());
        }
        None
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

    fn stream_chat(
        &self,
        system: Option<&str>,
        messages: &[Message],
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        use std::io::BufRead;
        let mut body = Self::body(&self.model, system, messages);
        body["stream"] = serde_json::Value::Bool(true);
        let resp = match ureq::post(API_URL)
            .set("x-api-key", &self.key)
            .set("anthropic-version", "2023-06-01")
            .set("content-type", "application/json")
            .send_json(body)
        {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                return Err(format!(
                    "anthropic HTTP {code}: {}",
                    r.into_string().unwrap_or_default().trim()
                ))
            }
            Err(e) => return Err(format!("anthropic: {e}")),
        };
        let mut full = String::new();
        let reader = std::io::BufReader::new(resp.into_reader());
        for line in reader.lines() {
            let line = line.map_err(|e| format!("anthropic: stream read: {e}"))?;
            if let Some(data) = line.strip_prefix("data: ") {
                if let Some(delta) = Self::sse_delta(data) {
                    on_delta(&delta);
                    full.push_str(&delta);
                }
            }
        }
        Ok(full)
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn agent_turn(
        &self,
        system: Option<&str>,
        turns: &[Turn],
        tools: &[Tool],
    ) -> Result<AssistantReply, String> {
        let tools_json: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect();
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "messages": Self::turns_json(turns),
            "tools": tools_json,
        });
        if let Some(sys) = system {
            body["system"] = serde_json::Value::String(sys.to_string());
        }
        let resp = read_response(
            ureq::post(API_URL)
                .set("x-api-key", &self.key)
                .set("anthropic-version", "2023-06-01")
                .set("content-type", "application/json")
                .send_json(body),
            "anthropic",
        )?;
        Self::parse_reply(&resp)
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

    #[test]
    fn sse_delta_extracts_text_delta_only() {
        let d =
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#;
        assert_eq!(Anthropic::sse_delta(d).as_deref(), Some("hi"));
        // non-text events yield nothing
        assert_eq!(Anthropic::sse_delta(r#"{"type":"message_start"}"#), None);
        assert_eq!(Anthropic::sse_delta(r#"{"type":"ping"}"#), None);
    }
}
