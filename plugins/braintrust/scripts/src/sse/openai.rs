use serde_json::Value;
use super::{SseEvent, StreamAction, StopReason};

// ============================================================================
// OpenAI Responses API SSE Parser (for participant)
// ============================================================================

/// Parse OpenAI Responses API SSE event into StreamAction.
pub fn parse_openai_responses_sse(event: &SseEvent) -> Option<StreamAction> {
    let data: Value = if event.data.is_empty() || event.data == "{}" {
        Value::Null
    } else {
        serde_json::from_str(&event.data).ok()?
    };

    match event.event_type.as_str() {
        "response.output_item.added" => {
            let index = data.get("output_index")?.as_u64()? as usize;
            let item = data.get("item")?;
            let item_type = item.get("type")?.as_str()?;

            match item_type {
                "function_call" => {
                    let id = item.get("call_id").and_then(|c| c.as_str()).unwrap_or("").to_string();
                    let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                    Some(StreamAction::ToolUseStart { index, id, name, thought_signature: None })
                }
                _ => None, // message, reasoning — handled via deltas
            }
        }

        "response.output_text.delta" => {
            let index = data.get("output_index")?.as_u64()? as usize;
            let delta = data.get("delta")?.as_str()?.to_string();
            Some(StreamAction::TextDelta { index, text: delta })
        }

        "response.function_call_arguments.delta" => {
            let index = data.get("output_index")?.as_u64()? as usize;
            let delta = data.get("delta")?.as_str()?.to_string();
            Some(StreamAction::InputJsonDelta { index, partial_json: delta })
        }

        "response.function_call_arguments.done" => {
            let index = data.get("output_index")?.as_u64()? as usize;
            let arguments = data.get("arguments").and_then(|a| a.as_str()).unwrap_or("").to_string();
            Some(StreamAction::InputJsonFinal { index, json: arguments })
        }

        "response.output_item.done" => {
            let index = data.get("output_index")?.as_u64()? as usize;
            let item = data.get("item")?;
            let item_type = item.get("type")?.as_str()?;

            match item_type {
                "message" | "reasoning" => Some(StreamAction::ContentBlockStop { index }),
                _ => None,
            }
        }

        "response.completed" => {
            let response = data.get("response")?;
            let status = response.get("status").and_then(|s| s.as_str()).unwrap_or("completed");

            let stop_reason = match status {
                "completed" => StopReason::EndTurn,
                "incomplete" => {
                    let reason = response.get("incomplete_details")
                        .and_then(|d| d.get("reason"))
                        .and_then(|r| r.as_str())
                        .unwrap_or("");
                    match reason {
                        "max_output_tokens" => StopReason::MaxTokens,
                        _ => StopReason::EndTurn,
                    }
                }
                _ => StopReason::Unknown,
            };

            // Check for function calls → ToolUse stop reason
            let has_function_calls = response.get("output")
                .and_then(|o| o.as_array())
                .map(|arr| arr.iter().any(|item| {
                    item.get("type").and_then(|t| t.as_str()) == Some("function_call")
                }))
                .unwrap_or(false);

            let final_stop_reason = if has_function_calls {
                StopReason::ToolUse
            } else {
                stop_reason
            };

            Some(StreamAction::MessageComplete { stop_reason: final_stop_reason })
        }

        "error" => {
            let msg = data.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .or_else(|| data.get("message").and_then(|m| m.as_str()))
                .unwrap_or("Unknown OpenAI SSE error");
            Some(StreamAction::Error(msg.to_string()))
        }

        _ => None,
    }
}

// ============================================================================
// OpenAI Chat Completions API SSE Parser (for chair)
// ============================================================================

/// Parse OpenAI Chat Completions SSE event into StreamAction.
pub fn parse_openai_chat_sse(event: &SseEvent) -> Option<StreamAction> {
    // Chat Completions SSE uses no event_type, just data lines.
    if event.data == "[DONE]" {
        return Some(StreamAction::MessageComplete { stop_reason: StopReason::EndTurn });
    }

    let data: Value = serde_json::from_str(&event.data).ok()?;

    // Check for error
    if let Some(error) = data.get("error") {
        let msg = error.get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown OpenAI chat SSE error");
        return Some(StreamAction::Error(msg.to_string()));
    }

    let choice = data.get("choices")?.get(0)?;
    let delta = choice.get("delta")?;

    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            return Some(StreamAction::TextDelta { index: 0, text: content.to_string() });
        }
    }

    // Check finish_reason
    if let Some(finish) = choice.get("finish_reason").and_then(|f| f.as_str()) {
        let stop_reason = match finish {
            "stop" => StopReason::EndTurn,
            "length" => StopReason::MaxTokens,
            _ => StopReason::Unknown,
        };
        return Some(StreamAction::MessageComplete { stop_reason });
    }

    None
}
