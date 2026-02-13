use serde_json::Value;
use super::{SseEvent, StreamAction, StopReason};

/// Parse Anthropic Messages API SSE event into StreamAction.
pub fn parse_anthropic_sse(event: &SseEvent) -> Option<StreamAction> {
    match event.event_type.as_str() {
        "content_block_start" => {
            let json: Value = serde_json::from_str(&event.data).ok()?;
            let index = json.get("index")?.as_u64()? as usize;
            let block = json.get("content_block")?;
            let block_type = block.get("type")?.as_str()?;

            match block_type {
                "tool_use" => {
                    let id = block.get("id")?.as_str()?.to_string();
                    let name = block.get("name")?.as_str()?.to_string();
                    Some(StreamAction::ToolUseStart { index, id, name, thought_signature: None })
                }
                _ => None, // text, thinking — handled via deltas
            }
        }

        "content_block_delta" => {
            let json: Value = serde_json::from_str(&event.data).ok()?;
            let index = json.get("index")?.as_u64()? as usize;
            let delta = json.get("delta")?;
            let delta_type = delta.get("type")?.as_str()?;

            match delta_type {
                "text_delta" => {
                    let text = delta.get("text")?.as_str()?.to_string();
                    Some(StreamAction::TextDelta { index, text })
                }
                "input_json_delta" => {
                    let partial = delta.get("partial_json")?.as_str()?.to_string();
                    Some(StreamAction::InputJsonDelta { index, partial_json: partial })
                }
                _ => None, // thinking_delta — ignored
            }
        }

        "content_block_stop" => {
            let json: Value = serde_json::from_str(&event.data).ok()?;
            let index = json.get("index")?.as_u64()? as usize;
            Some(StreamAction::ContentBlockStop { index })
        }

        "message_delta" => {
            let json: Value = serde_json::from_str(&event.data).ok()?;
            let delta = json.get("delta")?;
            let stop_reason_str = delta.get("stop_reason")?.as_str()?;
            let stop_reason = match stop_reason_str {
                "end_turn" => StopReason::EndTurn,
                "tool_use" => StopReason::ToolUse,
                "max_tokens" => StopReason::MaxTokens,
                _ => StopReason::Unknown,
            };
            Some(StreamAction::MessageComplete { stop_reason })
        }

        "message_start" | "message_stop" => None,
        "ping" => Some(StreamAction::Ping),

        "error" => {
            let json: Value = serde_json::from_str(&event.data).ok()?;
            let msg = json.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown Anthropic SSE error");
            Some(StreamAction::Error(msg.to_string()))
        }

        _ => None,
    }
}
