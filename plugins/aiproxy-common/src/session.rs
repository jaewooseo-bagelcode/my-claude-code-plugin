use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Shared types (extracted from braintrust session.rs)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiResponse {
    pub provider: String,
    pub content: String,
    pub model: String,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantStep {
    pub step: usize,
    pub step_type: String,
    pub tool_name: Option<String>,
    pub tool_input: Option<Value>,
    pub tool_output: Option<String>,
    pub tool_error: Option<String>,
    pub content: Option<String>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantSession {
    pub provider: String,
    pub model: String,
    pub steps: Vec<ParticipantStep>,
    pub final_content: String,
    pub success: bool,
    pub error: Option<String>,
}

impl ParticipantSession {
    pub fn new(provider: &str, model: &str) -> Self {
        Self {
            provider: provider.to_string(),
            model: model.to_string(),
            steps: Vec::new(),
            final_content: String::new(),
            success: false,
            error: None,
        }
    }

    pub fn add_tool_call(
        &mut self,
        tool_name: &str,
        tool_input: Value,
        tool_output: Result<String, String>,
    ) {
        let timestamp = now_millis();
        let (output, error) = match tool_output {
            Ok(out) => (Some(out), None),
            Err(err) => (None, Some(err)),
        };

        self.steps.push(ParticipantStep {
            step: self.steps.len() + 1,
            step_type: "tool_call".to_string(),
            tool_name: Some(tool_name.to_string()),
            tool_input: Some(tool_input),
            tool_output: output,
            tool_error: error,
            content: None,
            timestamp,
        });
    }

    pub fn finalize(&mut self, final_content: String, success: bool, error: Option<String>) {
        self.final_content = final_content;
        self.success = success;
        self.error = error;
    }

    pub fn to_ai_response(&self) -> AiResponse {
        AiResponse {
            provider: self.provider.clone(),
            content: self.final_content.clone(),
            model: self.model.clone(),
            success: self.success,
            error: self.error.clone(),
        }
    }
}

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ============================================================================
// JSONL Logger (ported from Go logging.go)
// ============================================================================

#[derive(Serialize)]
struct LogEntry {
    ts: u64,
    event: String,
    iteration: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

/// Structured JSONL logger (goroutine-safe, nil-safe equivalent).
/// If creation fails, log() is a no-op.
pub struct JsonlLogger {
    file: Option<Mutex<fs::File>>,
}

impl JsonlLogger {
    /// Create a logger writing to the given path. Returns a logger with None on error (graceful).
    pub fn new(path: &str) -> Self {
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new);
        Self { file }
    }

    /// Write one JSONL entry. No-op if logger has no file.
    pub fn log(&self, event: &str, iteration: usize, data: Option<Value>) {
        let Some(ref file_mutex) = self.file else {
            return;
        };
        let entry = LogEntry {
            ts: now_millis(),
            event: event.to_string(),
            iteration,
            data,
        };
        let Ok(mut line) = serde_json::to_string(&entry) else {
            return;
        };
        line.push('\n');

        if let Ok(mut f) = file_mutex.lock() {
            let _ = f.write_all(line.as_bytes());
        }
    }

    /// Close the log file.
    pub fn close(&self) {
        if let Some(ref file_mutex) = self.file {
            if let Ok(mut f) = file_mutex.lock() {
                let _ = f.flush();
            }
        }
    }
}

/// Truncate argument strings to maxLen characters for log summaries.
pub fn summarize_args(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}
