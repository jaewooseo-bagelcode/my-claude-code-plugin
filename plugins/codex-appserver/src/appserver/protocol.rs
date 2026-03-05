//! JSON-RPC 2.0 types for the Codex App Server protocol.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 request (client → server).
#[derive(Debug, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 2.0 notification (client → server, no id).
#[derive(Debug, Serialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: &'static str,
    pub method: String,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 2.0 response (server → client).
#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse {
    pub id: Option<u64>,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl std::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON-RPC error {}: {}", self.code, self.message)
    }
}

/// A message received from the server (response or notification).
#[derive(Debug)]
pub enum ServerMessage {
    Response(JsonRpcResponse),
    Notification { method: String, params: Value },
}

/// Raw server-side JSON-RPC message (for deserialization).
#[derive(Debug, Deserialize)]
struct RawServerMessage {
    id: Option<u64>,
    method: Option<String>,
    result: Option<Value>,
    error: Option<JsonRpcError>,
    #[serde(default)]
    params: Value,
}

impl ServerMessage {
    /// Parse a JSON line from the server into a typed message.
    pub fn parse(line: &str) -> Result<Self, String> {
        let raw: RawServerMessage =
            serde_json::from_str(line).map_err(|e| format!("Invalid JSON: {e}"))?;

        if raw.id.is_some() {
            // Response (has id)
            Ok(ServerMessage::Response(JsonRpcResponse {
                id: raw.id,
                result: raw.result,
                error: raw.error,
            }))
        } else if let Some(method) = raw.method {
            // Notification (has method, no id)
            Ok(ServerMessage::Notification {
                method,
                params: raw.params,
            })
        } else {
            Err("Message has neither id nor method".to_string())
        }
    }
}

// --- Review output types (structured output from codex) ---

/// Structured review output matching the outputSchema.
#[derive(Debug, Deserialize, Serialize)]
pub struct ReviewOutput {
    pub findings: Vec<Finding>,
    pub score: u8,
    pub summary: String,
    pub strengths: Vec<String>,
}

/// A single review finding.
#[derive(Debug, Deserialize, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub dimension: Dimension,
    pub title: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub problem: String,
    pub suggestion: String,
}

/// Finding severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Critical => write!(f, "CRITICAL"),
            Severity::High => write!(f, "HIGH"),
            Severity::Medium => write!(f, "MEDIUM"),
            Severity::Low => write!(f, "LOW"),
        }
    }
}

/// Review dimension category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Dimension {
    Bugs,
    Security,
    Performance,
    CodeQuality,
    Refactoring,
}

impl std::fmt::Display for Dimension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Dimension::Bugs => write!(f, "Bugs"),
            Dimension::Security => write!(f, "Security"),
            Dimension::Performance => write!(f, "Performance"),
            Dimension::CodeQuality => write!(f, "CodeQuality"),
            Dimension::Refactoring => write!(f, "Refactoring"),
        }
    }
}

/// The outputSchema sent to codex to enforce structured review output.
pub fn review_output_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "findings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "severity": { "type": "string", "enum": ["CRITICAL","HIGH","MEDIUM","LOW"] },
                        "dimension": { "type": "string", "enum": ["Bugs","Security","Performance","CodeQuality","Refactoring"] },
                        "title": { "type": "string" },
                        "file": { "type": "string" },
                        "line": { "type": "integer" },
                        "problem": { "type": "string" },
                        "suggestion": { "type": "string" }
                    },
                    "required": ["severity","dimension","title","file","line","problem","suggestion"],
                    "additionalProperties": false
                }
            },
            "score": { "type": "integer", "minimum": 1, "maximum": 10 },
            "summary": { "type": "string" },
            "strengths": { "type": "array", "items": { "type": "string" } }
        },
        "required": ["findings","score","summary","strengths"],
        "additionalProperties": false
    })
}
