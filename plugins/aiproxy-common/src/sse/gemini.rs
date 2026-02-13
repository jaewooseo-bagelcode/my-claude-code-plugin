use std::collections::HashMap;
use serde_json::Value;
use super::{SseEvent, StreamAction, StopReason};

/// Gemini SSE parsing state (tracks across multiple chunks).
pub struct GeminiSseState {
    /// part_idx → block index mapping
    pub part_to_index: HashMap<usize, usize>,
    /// Known text parts (already emitted first TextDelta)
    text_parts: std::collections::HashSet<usize>,
    /// Known function calls (part_idx → generated call id)
    pub known_function_calls: HashMap<usize, String>,
    /// Gemini 3 Pro thoughtSignature per function call (part_idx → signature)
    pub thought_signatures: HashMap<usize, String>,
    /// Next block index counter
    next_index: usize,
}

impl GeminiSseState {
    pub fn new() -> Self {
        Self {
            part_to_index: HashMap::new(),
            text_parts: std::collections::HashSet::new(),
            known_function_calls: HashMap::new(),
            thought_signatures: HashMap::new(),
            next_index: 0,
        }
    }
}

/// Parse Gemini streamGenerateContent SSE event.
/// Returns Vec because a single Gemini chunk can produce multiple actions.
/// Text is INCREMENTAL (not cumulative) — each chunk's text is new content.
pub fn parse_gemini_sse(event: &SseEvent, state: &mut GeminiSseState) -> Vec<StreamAction> {
    let mut actions = Vec::new();

    let data: Value = match serde_json::from_str(&event.data) {
        Ok(v) => v,
        Err(_) => return actions,
    };

    // Error check
    if let Some(error) = data.get("error") {
        let msg = error.get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown Gemini error");
        actions.push(StreamAction::Error(msg.to_string()));
        return actions;
    }

    // Extract candidate
    let candidate = match data.get("candidates").and_then(|c| c.get(0)) {
        Some(c) => c,
        None => return actions,
    };

    let finish_reason = candidate.get("finishReason").and_then(|f| f.as_str()).unwrap_or("");

    // Extract parts
    let parts = candidate.get("content")
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array());

    if let Some(parts) = parts {
        for (part_idx, part) in parts.iter().enumerate() {
            // Text part — INCREMENTAL
            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                if !text.is_empty() {
                    if !state.text_parts.contains(&part_idx) {
                        let index = state.next_index;
                        state.next_index += 1;
                        state.part_to_index.insert(part_idx, index);
                        state.text_parts.insert(part_idx);
                    }
                    let index = state.part_to_index.get(&part_idx).copied().unwrap_or(part_idx);
                    actions.push(StreamAction::TextDelta {
                        index,
                        text: text.to_string(),
                    });
                }
            }

            // Function call part
            if let Some(fc) = part.get("functionCall") {
                let name = fc.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                if !name.is_empty() && !state.known_function_calls.contains_key(&part_idx) {
                    let index = state.next_index;
                    state.next_index += 1;
                    let id = uuid::Uuid::new_v4().to_string();
                    state.known_function_calls.insert(part_idx, id.clone());
                    state.part_to_index.insert(part_idx, index);

                    // Capture thoughtSignature (required by Gemini 3 Pro for tool round-trip)
                    let thought_signature = part.get("thoughtSignature")
                        .and_then(|ts| ts.as_str())
                        .map(|s| s.to_string());
                    if let Some(ref sig) = thought_signature {
                        state.thought_signatures.insert(part_idx, sig.clone());
                    }

                    actions.push(StreamAction::ToolUseStart {
                        index,
                        id,
                        name,
                        thought_signature,
                    });

                    // Gemini sends complete args in one shot
                    let args = fc.get("args").cloned().unwrap_or(serde_json::json!({}));
                    let args_str = serde_json::to_string(&args).unwrap_or_default();
                    actions.push(StreamAction::InputJsonDelta {
                        index,
                        partial_json: args_str,
                    });
                }
            }
        }
    }

    // Handle finish reason
    if !finish_reason.is_empty() {
        // Close all open blocks
        for &part_idx in &state.text_parts {
            let index = state.part_to_index.get(&part_idx).copied().unwrap_or(part_idx);
            actions.push(StreamAction::ContentBlockStop { index });
        }
        for (&part_idx, _) in &state.known_function_calls {
            if let Some(&index) = state.part_to_index.get(&part_idx) {
                actions.push(StreamAction::ContentBlockStop { index });
            }
        }

        let stop_reason = match finish_reason {
            "STOP" => StopReason::EndTurn,
            "MAX_TOKENS" => StopReason::MaxTokens,
            "SAFETY" | "RECITATION" | "PROHIBITED_CONTENT" | "BLOCKLIST" => {
                eprintln!("[Gemini] Content blocked: {}", finish_reason);
                StopReason::EndTurn
            }
            _ => StopReason::Unknown,
        };

        let has_tool_calls = !state.known_function_calls.is_empty();
        let final_stop = if has_tool_calls {
            StopReason::ToolUse
        } else {
            stop_reason
        };

        actions.push(StreamAction::MessageComplete { stop_reason: final_stop });
    }

    actions
}
