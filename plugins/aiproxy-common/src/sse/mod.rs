pub mod anthropic;
pub mod gemini;
pub mod openai;
pub mod streaming;

use std::time::Duration;

/// Idle timeout for SSE streams — if no data received for this duration, consider the stream dead.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

// ============================================================================
// SSE Parser — Provider-agnostic byte-to-event parser
// ============================================================================

pub struct SseParser {
    buffer: String,
}

impl SseParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    /// Feed a byte chunk and return any complete SSE events.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let text = String::from_utf8_lossy(chunk);
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        self.buffer.push_str(&normalized);

        let mut events = Vec::new();

        while let Some(pos) = self.buffer.find("\n\n") {
            let raw_event = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 2..].to_string();

            if let Some(event) = Self::parse_raw_event(&raw_event) {
                events.push(event);
            }
        }

        events
    }

    /// Flush remaining buffer after stream ends (handles missing trailing \n\n).
    pub fn flush(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();
        let remaining = std::mem::take(&mut self.buffer);
        let trimmed = remaining.trim();
        if !trimmed.is_empty() {
            if let Some(event) = Self::parse_raw_event(trimmed) {
                events.push(event);
            }
        }
        events
    }

    fn parse_raw_event(raw: &str) -> Option<SseEvent> {
        let mut event_type = String::new();
        let mut data = String::new();

        for line in raw.lines() {
            if let Some(value) = line.strip_prefix("event: ") {
                event_type = value.trim().to_string();
            } else if let Some(value) = line.strip_prefix("data: ") {
                data = value.to_string();
            } else if line.starts_with("data:") {
                data = line[5..].to_string();
            }
        }

        if event_type.is_empty() && data.is_empty() {
            return None;
        }

        Some(SseEvent { event_type, data })
    }
}

#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event_type: String,
    pub data: String,
}

// ============================================================================
// StreamAction — Normalized action types across all providers
// ============================================================================

#[derive(Debug)]
pub enum StreamAction {
    TextDelta { index: usize, text: String },
    ToolUseStart { index: usize, id: String, name: String, thought_signature: Option<String> },
    InputJsonDelta { index: usize, partial_json: String },
    /// Complete final JSON from OpenAI .done event — replaces delta buffer.
    InputJsonFinal { index: usize, json: String },
    ContentBlockStop { index: usize },
    MessageComplete { stop_reason: StopReason },
    Error(String),
    Ping,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Unknown,
}
