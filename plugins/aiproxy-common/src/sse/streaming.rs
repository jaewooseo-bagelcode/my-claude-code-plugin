//! High-level SSE streaming functions for all AI providers.
//!
//! Each function handles POST → status check → SSE loop with idle timeout →
//! accumulate into StreamResult. Consumers never touch raw bytes or SSE events.

use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use tokio::time::timeout;

use super::anthropic::parse_anthropic_sse;
use super::gemini::{parse_gemini_sse, GeminiSseState};
use super::openai::{parse_openai_chat_sse, parse_openai_responses_sse};
use super::{SseParser, StreamAction, StopReason, IDLE_TIMEOUT};

// ============================================================================
// Public types
// ============================================================================

#[derive(Debug, Clone)]
pub struct StreamResult {
    /// OpenAI Responses API only — response ID for session chaining.
    pub response_id: Option<String>,
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: StopReason,
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
    /// Gemini only — required for tool round-trip in Gemini 3 Pro.
    pub thought_signature: Option<String>,
}

// ============================================================================
// StreamResult accumulator (shared logic for all providers)
// ============================================================================

/// Internal accumulator that converts a sequence of StreamActions into StreamResult.
pub struct StreamAccumulator {
    pub text: String,
    // (index, id, name, args_buffer, thought_signature)
    tool_calls: Vec<(usize, String, String, String, Option<String>)>,
    pub stop_reason: StopReason,
    pub response_id: Option<String>,
    had_error: bool,
}

impl StreamAccumulator {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            tool_calls: Vec::new(),
            stop_reason: StopReason::Unknown,
            response_id: None,
            had_error: false,
        }
    }

    /// Process a single StreamAction.
    pub fn process(&mut self, action: StreamAction) {
        match action {
            StreamAction::TextDelta { text, .. } => {
                self.text.push_str(&text);
            }
            StreamAction::ToolUseStart { index, id, name, thought_signature } => {
                self.tool_calls.push((index, id, name, String::new(), thought_signature));
            }
            StreamAction::InputJsonDelta { index, partial_json } => {
                if let Some(tc) = self.find_tool_call_mut(index) {
                    tc.3.push_str(&partial_json);
                }
            }
            StreamAction::InputJsonFinal { index, json } => {
                // Replace delta buffer with final JSON
                if let Some(tc) = self.find_tool_call_mut(index) {
                    tc.3 = json;
                }
            }
            StreamAction::MessageComplete { stop_reason } => {
                self.stop_reason = stop_reason;
            }
            StreamAction::ContentBlockStop { .. } | StreamAction::Ping => {}
            StreamAction::Error(msg) => {
                // Treat SSE-level errors as stop with error text
                self.had_error = true;
                if self.text.is_empty() {
                    self.text = format!("[SSE Error] {}", msg);
                }
                self.stop_reason = StopReason::Unknown;
            }
        }
    }

    fn find_tool_call_mut(&mut self, index: usize) -> Option<&mut (usize, String, String, String, Option<String>)> {
        self.tool_calls.iter_mut().rfind(|tc| tc.0 == index)
    }

    pub fn into_result(self) -> StreamResult {
        // Infer stop_reason when provider didn't send MessageComplete.
        // Don't infer on error — Unknown is correct for error cases.
        let stop_reason = if self.stop_reason == StopReason::Unknown && !self.had_error {
            if !self.tool_calls.is_empty() {
                StopReason::ToolUse
            } else if !self.text.is_empty() {
                StopReason::EndTurn
            } else {
                StopReason::Unknown
            }
        } else {
            self.stop_reason
        };

        StreamResult {
            response_id: self.response_id,
            text: self.text,
            tool_calls: self.tool_calls
                .into_iter()
                .map(|(_, id, name, args, sig)| ToolCall {
                    id,
                    name,
                    arguments: args,
                    thought_signature: sig,
                })
                .collect(),
            stop_reason,
        }
    }
}

// ============================================================================
// OpenAI Responses API (codex-review consumer)
// ============================================================================

/// Extract response_id from a "response.created" SSE event.
fn extract_response_id(event: &super::SseEvent) -> Option<String> {
    if event.event_type != "response.created" {
        return None;
    }
    serde_json::from_str::<Value>(&event.data)
        .ok()?
        .get("response")?
        .get("id")?
        .as_str()
        .map(String::from)
}

/// Stream OpenAI Responses API (`/v1/responses`).
///
/// Extracts `response_id` from `response.created` event for session chaining.
/// Returns accumulated text, tool calls, and stop reason.
pub async fn stream_openai_responses(
    client: &Client,
    url: &str,
    auth_token: &str,
    mut payload: Value,
) -> Result<StreamResult, Box<dyn std::error::Error + Send + Sync>> {
    payload["stream"] = Value::Bool(true);

    let response = client
        .post(url)
        .bearer_auth(auth_token)
        .json(&payload)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("OpenAI Responses API error HTTP {}: {}", status, body).into());
    }

    let mut parser = SseParser::new();
    let mut byte_stream = response.bytes_stream();
    let mut acc = StreamAccumulator::new();

    loop {
        match timeout(IDLE_TIMEOUT, byte_stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                for event in parser.feed(&chunk) {
                    if let Some(id) = extract_response_id(&event) {
                        acc.response_id = Some(id);
                    }
                    if let Some(action) = parse_openai_responses_sse(&event) {
                        acc.process(action);
                    }
                }
            }
            Ok(Some(Err(e))) => {
                return Err(format!("OpenAI stream error: {}", e).into());
            }
            Ok(None) => break,
            Err(_) => {
                if !acc.text.is_empty() || !acc.tool_calls.is_empty() {
                    break;
                }
                return Err("OpenAI Responses stream idle timeout (60s)".into());
            }
        }
    }

    for event in parser.flush() {
        if let Some(id) = extract_response_id(&event) {
            acc.response_id = Some(id);
        }
        if let Some(action) = parse_openai_responses_sse(&event) {
            acc.process(action);
        }
    }

    Ok(acc.into_result())
}

// ============================================================================
// OpenAI Chat Completions API (braintrust chair consumer)
// ============================================================================

/// Stream OpenAI Chat Completions API (`/v1/chat/completions`).
///
/// Text-only streaming with `[DONE]` termination.
pub async fn stream_openai_chat(
    client: &Client,
    url: &str,
    auth_token: &str,
    mut payload: Value,
) -> Result<StreamResult, Box<dyn std::error::Error + Send + Sync>> {
    payload["stream"] = Value::Bool(true);

    let response = client
        .post(url)
        .bearer_auth(auth_token)
        .json(&payload)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("OpenAI Chat API error HTTP {}: {}", status, body).into());
    }

    let mut parser = SseParser::new();
    let mut byte_stream = response.bytes_stream();
    let mut acc = StreamAccumulator::new();

    loop {
        match timeout(IDLE_TIMEOUT, byte_stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                for event in parser.feed(&chunk) {
                    if let Some(action) = parse_openai_chat_sse(&event) {
                        acc.process(action);
                    }
                }
            }
            Ok(Some(Err(e))) => {
                return Err(format!("OpenAI Chat stream error: {}", e).into());
            }
            Ok(None) => break,
            Err(_) => {
                if !acc.text.is_empty() {
                    break;
                }
                return Err("OpenAI Chat stream idle timeout (60s)".into());
            }
        }
    }

    for event in parser.flush() {
        if let Some(action) = parse_openai_chat_sse(&event) {
            acc.process(action);
        }
    }

    Ok(acc.into_result())
}

// ============================================================================
// Anthropic Messages API (braintrust claude consumer)
// ============================================================================

/// Stream Anthropic Messages API (`/v1/messages`).
///
/// Multi-block streaming with content_block_start/delta/stop + message_delta.
pub async fn stream_anthropic(
    client: &Client,
    url: &str,
    auth_header: &str,
    auth_value: &str,
    mut payload: Value,
) -> Result<StreamResult, Box<dyn std::error::Error + Send + Sync>> {
    payload["stream"] = Value::Bool(true);

    let response = client
        .post(url)
        .header(auth_header, auth_value)
        .header("anthropic-version", "2023-06-01")
        .json(&payload)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Anthropic API error HTTP {}: {}", status, body).into());
    }

    let mut parser = SseParser::new();
    let mut byte_stream = response.bytes_stream();
    let mut acc = StreamAccumulator::new();

    loop {
        match timeout(IDLE_TIMEOUT, byte_stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                for event in parser.feed(&chunk) {
                    if let Some(action) = parse_anthropic_sse(&event) {
                        acc.process(action);
                    }
                }
            }
            Ok(Some(Err(e))) => {
                return Err(format!("Anthropic stream error: {}", e).into());
            }
            Ok(None) => break,
            Err(_) => {
                if !acc.text.is_empty() || !acc.tool_calls.is_empty() {
                    break;
                }
                return Err("Anthropic stream idle timeout (60s)".into());
            }
        }
    }

    for event in parser.flush() {
        if let Some(action) = parse_anthropic_sse(&event) {
            acc.process(action);
        }
    }

    Ok(acc.into_result())
}

// ============================================================================
// Gemini streamGenerateContent API (braintrust gemini consumer)
// ============================================================================

/// Stream Gemini streamGenerateContent API.
///
/// URL should already include `?alt=sse` suffix. Uses GeminiSseState for
/// stateful parsing across chunks.
pub async fn stream_gemini(
    client: &Client,
    url: &str,
    auth_header: &str,
    auth_value: &str,
    payload: Value,
) -> Result<StreamResult, Box<dyn std::error::Error + Send + Sync>> {
    let response = client
        .post(url)
        .header(auth_header, auth_value)
        .json(&payload)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Gemini API error HTTP {}: {}", status, body).into());
    }

    let mut parser = SseParser::new();
    let mut byte_stream = response.bytes_stream();
    let mut state = GeminiSseState::new();
    let mut acc = StreamAccumulator::new();

    loop {
        match timeout(IDLE_TIMEOUT, byte_stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                for event in parser.feed(&chunk) {
                    for action in parse_gemini_sse(&event, &mut state) {
                        acc.process(action);
                    }
                }
            }
            Ok(Some(Err(e))) => {
                return Err(format!("Gemini stream error: {}", e).into());
            }
            Ok(None) => break,
            Err(_) => {
                if !acc.text.is_empty() || !acc.tool_calls.is_empty() {
                    break;
                }
                return Err("Gemini stream idle timeout (60s)".into());
            }
        }
    }

    for event in parser.flush() {
        for action in parse_gemini_sse(&event, &mut state) {
            acc.process(action);
        }
    }

    Ok(acc.into_result())
}
