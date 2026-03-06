//! E2E tests for the appserver module.
//! Tests protocol types, serialization, deserialization, review output parsing,
//! and coder output parsing.
//! Does NOT require a running codex process — tests the client library logic only.

use codex_appserver::appserver::client::ShutdownStatus;
use codex_appserver::appserver::protocol::{
    coder_output_schema, review_output_schema, CoderOutput, CoderStatus, Dimension, FileAction,
    FileChange, Finding, JsonRpcError, JsonRpcNotification, JsonRpcRequest, ReviewOutput,
    ServerMessage, Severity,
};
use serde_json::{json, Value};

// ============================================================================
// JsonRpcRequest — serialization
// ============================================================================

#[test]
fn request_serializes_correctly() {
    let req = JsonRpcRequest::new(1, "initialize", json!({"clientInfo": {"name": "test"}}));
    let s = serde_json::to_string(&req).unwrap();
    let parsed: Value = serde_json::from_str(&s).unwrap();

    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["id"], 1);
    assert_eq!(parsed["method"], "initialize");
    assert_eq!(parsed["params"]["clientInfo"]["name"], "test");
}

#[test]
fn request_skips_null_params() {
    let req = JsonRpcRequest::new(2, "shutdown", Value::Null);
    let s = serde_json::to_string(&req).unwrap();
    let parsed: Value = serde_json::from_str(&s).unwrap();

    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["id"], 2);
    assert_eq!(parsed["method"], "shutdown");
    // params should be absent (skip_serializing_if = is_null)
    assert!(parsed.get("params").is_none());
}

#[test]
fn request_preserves_complex_params() {
    let params = json!({
        "threadId": "thr_abc123",
        "input": [{"type": "text", "text": "Review this code"}],
        "outputSchema": {"type": "object", "properties": {"score": {"type": "integer"}}}
    });
    let req = JsonRpcRequest::new(42, "turn/start", params.clone());
    let s = serde_json::to_string(&req).unwrap();
    let parsed: Value = serde_json::from_str(&s).unwrap();

    assert_eq!(parsed["params"], params);
}

// ============================================================================
// JsonRpcNotification — serialization
// ============================================================================

#[test]
fn notification_serializes_without_id() {
    let notif = JsonRpcNotification::new("initialized", Value::Null);
    let s = serde_json::to_string(&notif).unwrap();
    let parsed: Value = serde_json::from_str(&s).unwrap();

    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["method"], "initialized");
    assert!(parsed.get("id").is_none());
    assert!(parsed.get("params").is_none());
}

#[test]
fn notification_with_params() {
    let notif = JsonRpcNotification::new("exit", json!({"reason": "done"}));
    let s = serde_json::to_string(&notif).unwrap();
    let parsed: Value = serde_json::from_str(&s).unwrap();

    assert_eq!(parsed["params"]["reason"], "done");
}

// ============================================================================
// ServerMessage::parse — response parsing
// ============================================================================

#[test]
fn parse_response_with_result() {
    let line = r#"{"jsonrpc":"2.0","id":1,"result":{"id":"thr_123","status":"active"}}"#;
    let msg = ServerMessage::parse(line).unwrap();

    match msg {
        ServerMessage::Response(resp) => {
            assert_eq!(resp.id, Some(1));
            assert!(resp.error.is_none());
            let result = resp.result.unwrap();
            assert_eq!(result["id"], "thr_123");
        }
        _ => panic!("Expected Response, got Notification"),
    }
}

#[test]
fn parse_response_with_error() {
    let line = r#"{"jsonrpc":"2.0","id":5,"error":{"code":-32600,"message":"Invalid request","data":null}}"#;
    let msg = ServerMessage::parse(line).unwrap();

    match msg {
        ServerMessage::Response(resp) => {
            assert_eq!(resp.id, Some(5));
            let err = resp.error.unwrap();
            assert_eq!(err.code, -32600);
            assert_eq!(err.message, "Invalid request");
        }
        _ => panic!("Expected Response"),
    }
}

#[test]
fn parse_response_with_null_result() {
    let line = r#"{"jsonrpc":"2.0","id":10,"result":null}"#;
    let msg = ServerMessage::parse(line).unwrap();

    match msg {
        ServerMessage::Response(resp) => {
            assert_eq!(resp.id, Some(10));
            assert!(resp.error.is_none());
            // serde deserializes JSON null as None for Option<Value>
            assert!(resp.result.is_none());
        }
        _ => panic!("Expected Response"),
    }
}

// ============================================================================
// ServerMessage::parse — notification parsing
// ============================================================================

#[test]
fn parse_agent_message_delta_notification() {
    let line = r#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"itemId":"item_1","delta":"Hello "}}"#;
    let msg = ServerMessage::parse(line).unwrap();

    match msg {
        ServerMessage::Notification { method, params } => {
            assert_eq!(method, "item/agentMessage/delta");
            assert_eq!(params["delta"], "Hello ");
            assert_eq!(params["itemId"], "item_1");
        }
        _ => panic!("Expected Notification"),
    }
}

#[test]
fn parse_turn_completed_notification() {
    let line = r#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"turn_1","status":"completed"}}}"#;
    let msg = ServerMessage::parse(line).unwrap();

    match msg {
        ServerMessage::Notification { method, params } => {
            assert_eq!(method, "turn/completed");
            assert_eq!(params["turn"]["status"], "completed");
        }
        _ => panic!("Expected Notification"),
    }
}

#[test]
fn parse_item_started_notification() {
    let line = r#"{"jsonrpc":"2.0","method":"item/started","params":{"itemId":"item_abc","type":"agentMessage"}}"#;
    let msg = ServerMessage::parse(line).unwrap();

    match msg {
        ServerMessage::Notification { method, params } => {
            assert_eq!(method, "item/started");
            assert_eq!(params["type"], "agentMessage");
        }
        _ => panic!("Expected Notification"),
    }
}

#[test]
fn parse_turn_completed_with_failure() {
    let line = r#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"turn_2","status":"failed","error":{"message":"Model timeout"}}}}"#;
    let msg = ServerMessage::parse(line).unwrap();

    match msg {
        ServerMessage::Notification { method, params } => {
            assert_eq!(method, "turn/completed");
            assert_eq!(params["turn"]["status"], "failed");
            assert_eq!(params["turn"]["error"]["message"], "Model timeout");
        }
        _ => panic!("Expected Notification"),
    }
}

// ============================================================================
// ServerMessage::parse — error cases
// ============================================================================

#[test]
fn parse_invalid_json_returns_error() {
    let result = ServerMessage::parse("not json at all");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Invalid JSON"));
}

#[test]
fn parse_empty_object_returns_error() {
    let result = ServerMessage::parse(r#"{"jsonrpc":"2.0"}"#);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("neither id nor method"));
}

#[test]
fn parse_empty_string_returns_error() {
    let result = ServerMessage::parse("");
    assert!(result.is_err());
}

// ============================================================================
// JsonRpcError — Display
// ============================================================================

#[test]
fn json_rpc_error_display_format() {
    let err_json = r#"{"code":-32001,"message":"Server overloaded","data":null}"#;
    let err: JsonRpcError = serde_json::from_str(err_json).unwrap();
    assert_eq!(format!("{err}"), "JSON-RPC error -32001: Server overloaded");
}

// ============================================================================
// ReviewOutput — deserialization
// ============================================================================

#[test]
fn review_output_full_deserialize() {
    let json_str = r#"{
        "findings": [
            {
                "severity": "HIGH",
                "dimension": "Bugs",
                "title": "Null pointer dereference",
                "file": "src/main.rs",
                "line": 42,
                "problem": "Unwrap on None value",
                "suggestion": "Use if-let or match"
            },
            {
                "severity": "LOW",
                "dimension": "CodeQuality",
                "title": "Magic number",
                "file": "src/config.rs",
                "problem": "Hardcoded timeout value",
                "suggestion": "Extract to constant"
            }
        ],
        "score": 7,
        "summary": "Generally good code with one critical bug",
        "strengths": ["Good error handling", "Clean module structure"]
    }"#;

    let review: ReviewOutput = serde_json::from_str(json_str).unwrap();
    assert_eq!(review.score, 7);
    assert_eq!(review.findings.len(), 2);
    assert_eq!(review.strengths.len(), 2);
    assert_eq!(review.summary, "Generally good code with one critical bug");

    // First finding
    assert_eq!(review.findings[0].severity, Severity::High);
    assert_eq!(review.findings[0].dimension, Dimension::Bugs);
    assert_eq!(review.findings[0].line, Some(42));

    // Second finding — no line number
    assert_eq!(review.findings[1].severity, Severity::Low);
    assert_eq!(review.findings[1].dimension, Dimension::CodeQuality);
    assert_eq!(review.findings[1].line, None);
}

#[test]
fn review_output_empty_findings() {
    let json_str = r#"{
        "findings": [],
        "score": 10,
        "summary": "Perfect code",
        "strengths": ["Everything is great"]
    }"#;

    let review: ReviewOutput = serde_json::from_str(json_str).unwrap();
    assert_eq!(review.score, 10);
    assert!(review.findings.is_empty());
    assert_eq!(review.strengths.len(), 1);
}

#[test]
fn review_output_all_severities() {
    let json_str = r#"{
        "findings": [
            {"severity": "CRITICAL", "dimension": "Security", "title": "SQL injection", "file": "db.rs", "problem": "p", "suggestion": "s"},
            {"severity": "HIGH", "dimension": "Bugs", "title": "Buffer overflow", "file": "buf.rs", "problem": "p", "suggestion": "s"},
            {"severity": "MEDIUM", "dimension": "Performance", "title": "N+1 query", "file": "api.rs", "problem": "p", "suggestion": "s"},
            {"severity": "LOW", "dimension": "Refactoring", "title": "Dead code", "file": "old.rs", "problem": "p", "suggestion": "s"}
        ],
        "score": 4,
        "summary": "Multiple issues found",
        "strengths": []
    }"#;

    let review: ReviewOutput = serde_json::from_str(json_str).unwrap();
    assert_eq!(review.findings[0].severity, Severity::Critical);
    assert_eq!(review.findings[1].severity, Severity::High);
    assert_eq!(review.findings[2].severity, Severity::Medium);
    assert_eq!(review.findings[3].severity, Severity::Low);
}

#[test]
fn review_output_all_dimensions() {
    let json_str = r#"{
        "findings": [
            {"severity": "LOW", "dimension": "Bugs", "title": "t", "file": "f", "problem": "p", "suggestion": "s"},
            {"severity": "LOW", "dimension": "Security", "title": "t", "file": "f", "problem": "p", "suggestion": "s"},
            {"severity": "LOW", "dimension": "Performance", "title": "t", "file": "f", "problem": "p", "suggestion": "s"},
            {"severity": "LOW", "dimension": "CodeQuality", "title": "t", "file": "f", "problem": "p", "suggestion": "s"},
            {"severity": "LOW", "dimension": "Refactoring", "title": "t", "file": "f", "problem": "p", "suggestion": "s"}
        ],
        "score": 5,
        "summary": "s",
        "strengths": []
    }"#;

    let review: ReviewOutput = serde_json::from_str(json_str).unwrap();
    assert_eq!(review.findings[0].dimension, Dimension::Bugs);
    assert_eq!(review.findings[1].dimension, Dimension::Security);
    assert_eq!(review.findings[2].dimension, Dimension::Performance);
    assert_eq!(review.findings[3].dimension, Dimension::CodeQuality);
    assert_eq!(review.findings[4].dimension, Dimension::Refactoring);
}

// ============================================================================
// ReviewOutput — serialization roundtrip
// ============================================================================

#[test]
fn review_output_roundtrip() {
    let review = ReviewOutput {
        findings: vec![Finding {
            severity: Severity::High,
            dimension: Dimension::Bugs,
            title: "Test bug".to_string(),
            file: "test.rs".to_string(),
            line: Some(10),
            problem: "Something wrong".to_string(),
            suggestion: "Fix it".to_string(),
        }],
        score: 8,
        summary: "Good overall".to_string(),
        strengths: vec!["Clean code".to_string()],
    };

    let json_str = serde_json::to_string(&review).unwrap();
    let deserialized: ReviewOutput = serde_json::from_str(&json_str).unwrap();

    assert_eq!(deserialized.score, review.score);
    assert_eq!(deserialized.summary, review.summary);
    assert_eq!(deserialized.findings.len(), 1);
    assert_eq!(deserialized.findings[0].title, "Test bug");
    assert_eq!(deserialized.findings[0].line, Some(10));
}

#[test]
fn review_output_skip_none_line_in_serialization() {
    let finding = Finding {
        severity: Severity::Low,
        dimension: Dimension::CodeQuality,
        title: "t".to_string(),
        file: "f".to_string(),
        line: None,
        problem: "p".to_string(),
        suggestion: "s".to_string(),
    };
    let json_str = serde_json::to_string(&finding).unwrap();
    let parsed: Value = serde_json::from_str(&json_str).unwrap();
    // line should be absent due to skip_serializing_if
    assert!(parsed.get("line").is_none());
}

// ============================================================================
// Severity / Dimension — Display
// ============================================================================

#[test]
fn severity_display() {
    assert_eq!(format!("{}", Severity::Critical), "CRITICAL");
    assert_eq!(format!("{}", Severity::High), "HIGH");
    assert_eq!(format!("{}", Severity::Medium), "MEDIUM");
    assert_eq!(format!("{}", Severity::Low), "LOW");
}

#[test]
fn dimension_display() {
    assert_eq!(format!("{}", Dimension::Bugs), "Bugs");
    assert_eq!(format!("{}", Dimension::Security), "Security");
    assert_eq!(format!("{}", Dimension::Performance), "Performance");
    assert_eq!(format!("{}", Dimension::CodeQuality), "CodeQuality");
    assert_eq!(format!("{}", Dimension::Refactoring), "Refactoring");
}

// ============================================================================
// review_output_schema — schema validation
// ============================================================================

#[test]
fn output_schema_has_required_fields() {
    let schema = review_output_schema();
    assert_eq!(schema["type"], "object");

    let required = schema["required"].as_array().unwrap();
    let req_strs: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(req_strs.contains(&"findings"));
    assert!(req_strs.contains(&"score"));
    assert!(req_strs.contains(&"summary"));
    assert!(req_strs.contains(&"strengths"));
}

#[test]
fn output_schema_score_has_bounds() {
    let schema = review_output_schema();
    let score = &schema["properties"]["score"];
    assert_eq!(score["type"], "integer");
    assert_eq!(score["minimum"], 1);
    assert_eq!(score["maximum"], 10);
}

#[test]
fn output_schema_findings_item_has_severity_enum() {
    let schema = review_output_schema();
    let severity = &schema["properties"]["findings"]["items"]["properties"]["severity"];
    assert_eq!(severity["type"], "string");
    let enums = severity["enum"].as_array().unwrap();
    assert_eq!(enums.len(), 4);
    assert!(enums.contains(&json!("CRITICAL")));
    assert!(enums.contains(&json!("HIGH")));
    assert!(enums.contains(&json!("MEDIUM")));
    assert!(enums.contains(&json!("LOW")));
}

#[test]
fn output_schema_findings_item_has_dimension_enum() {
    let schema = review_output_schema();
    let dimension = &schema["properties"]["findings"]["items"]["properties"]["dimension"];
    let enums = dimension["enum"].as_array().unwrap();
    assert_eq!(enums.len(), 5);
    assert!(enums.contains(&json!("Bugs")));
    assert!(enums.contains(&json!("Security")));
    assert!(enums.contains(&json!("Performance")));
    assert!(enums.contains(&json!("CodeQuality")));
    assert!(enums.contains(&json!("Refactoring")));
}

#[test]
fn output_schema_disallows_additional_properties() {
    let schema = review_output_schema();
    assert_eq!(schema["additionalProperties"], false);
}

// ============================================================================
// Protocol message stream simulation
// ============================================================================

#[test]
fn simulate_full_review_stream() {
    // Simulate a realistic sequence of server messages during a review turn
    let messages = vec![
        // Response to initialize
        r#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":"codex-app-server/0.1.0"}}"#,
        // Response to thread/start
        r#"{"jsonrpc":"2.0","id":2,"result":{"id":"thr_review_1","status":"active","createdAt":"2026-02-25T00:00:00Z"}}"#,
        // Response to turn/start
        r#"{"jsonrpc":"2.0","id":3,"result":{"id":"turn_1","status":"in_progress"}}"#,
        // Streaming deltas
        r#"{"jsonrpc":"2.0","method":"item/started","params":{"itemId":"item_1","type":"agentMessage"}}"#,
        r#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"itemId":"item_1","delta":"{\"findings\":[{\"severity\":\"HIGH\""}}"#,
        r#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"itemId":"item_1","delta":",\"dimension\":\"Bugs\",\"title\":\"test\",\"file\":\"f\",\"problem\":\"p\",\"suggestion\":\"s\"}]"}}"#,
        r#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"itemId":"item_1","delta":",\"score\":8,\"summary\":\"ok\",\"strengths\":[]}"}}"#,
        r#"{"jsonrpc":"2.0","method":"item/completed","params":{"itemId":"item_1"}}"#,
        // turn/completed
        r#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"turn_1","status":"completed"}}}"#,
    ];

    let mut agent_text = String::new();
    let mut turn_completed = false;
    let mut thread_id = String::new();

    for line in &messages {
        let msg = ServerMessage::parse(line).unwrap();
        match msg {
            ServerMessage::Response(resp) => {
                if let Some(result) = resp.result {
                    if let Some(id) = result.get("id").and_then(|v| v.as_str()) {
                        if id.starts_with("thr_") {
                            thread_id = id.to_string();
                        }
                    }
                }
            }
            ServerMessage::Notification { method, params } => match method.as_str() {
                "item/agentMessage/delta" => {
                    if let Some(delta) = params.get("delta").and_then(|d| d.as_str()) {
                        agent_text.push_str(delta);
                    }
                }
                "turn/completed" => {
                    turn_completed = true;
                }
                _ => {}
            },
        }
    }

    assert_eq!(thread_id, "thr_review_1");
    assert!(turn_completed);

    // Parse the accumulated text as ReviewOutput
    let review: ReviewOutput = serde_json::from_str(&agent_text).unwrap();
    assert_eq!(review.score, 8);
    assert_eq!(review.findings.len(), 1);
    assert_eq!(review.findings[0].severity, Severity::High);
    assert_eq!(review.summary, "ok");
}

// ============================================================================
// Edge cases
// ============================================================================

#[test]
fn parse_response_with_id_zero() {
    let line = r#"{"jsonrpc":"2.0","id":0,"result":{}}"#;
    let msg = ServerMessage::parse(line).unwrap();
    match msg {
        ServerMessage::Response(resp) => assert_eq!(resp.id, Some(0)),
        _ => panic!("Expected Response"),
    }
}

#[test]
fn parse_notification_with_empty_params() {
    let line = r#"{"jsonrpc":"2.0","method":"item/completed","params":{}}"#;
    let msg = ServerMessage::parse(line).unwrap();
    match msg {
        ServerMessage::Notification { method, params } => {
            assert_eq!(method, "item/completed");
            assert!(params.is_object());
        }
        _ => panic!("Expected Notification"),
    }
}

#[test]
fn parse_notification_without_params_field() {
    // Some notifications may omit params entirely
    let line = r#"{"jsonrpc":"2.0","method":"ping"}"#;
    let msg = ServerMessage::parse(line).unwrap();
    match msg {
        ServerMessage::Notification { method, params } => {
            assert_eq!(method, "ping");
            assert!(params.is_null());
        }
        _ => panic!("Expected Notification"),
    }
}

#[test]
fn review_output_rejects_invalid_severity() {
    let json_str = r#"{
        "findings": [{"severity": "INVALID", "dimension": "Bugs", "title": "t", "file": "f", "problem": "p", "suggestion": "s"}],
        "score": 5,
        "summary": "s",
        "strengths": []
    }"#;

    let result = serde_json::from_str::<ReviewOutput>(json_str);
    assert!(result.is_err());
}

#[test]
fn review_output_rejects_invalid_dimension() {
    let json_str = r#"{
        "findings": [{"severity": "HIGH", "dimension": "Unknown", "title": "t", "file": "f", "problem": "p", "suggestion": "s"}],
        "score": 5,
        "summary": "s",
        "strengths": []
    }"#;

    let result = serde_json::from_str::<ReviewOutput>(json_str);
    assert!(result.is_err());
}

#[test]
fn review_output_rejects_missing_required_fields() {
    let json_str = r#"{"findings": [], "score": 5}"#;
    let result = serde_json::from_str::<ReviewOutput>(json_str);
    assert!(result.is_err());
}

#[test]
fn backpressure_error_code_minus_32001() {
    let line = r#"{"jsonrpc":"2.0","id":99,"error":{"code":-32001,"message":"Server overloaded"}}"#;
    let msg = ServerMessage::parse(line).unwrap();
    match msg {
        ServerMessage::Response(resp) => {
            let err = resp.error.unwrap();
            assert_eq!(err.code, -32001);
            assert!(err.message.contains("overloaded"));
        }
        _ => panic!("Expected Response"),
    }
}

// ============================================================================
// ShutdownStatus — production hardening
// ============================================================================

#[test]
fn shutdown_status_all_ok_is_clean() {
    let status = ShutdownStatus {
        shutdown_request: Ok(()),
        exit_notify: Ok(()),
        process_exited: true,
    };
    assert!(status.is_clean());
}

#[test]
fn shutdown_status_request_error_is_not_clean() {
    let status = ShutdownStatus {
        shutdown_request: Err("timeout".to_string()),
        exit_notify: Ok(()),
        process_exited: true,
    };
    assert!(!status.is_clean());
}

#[test]
fn shutdown_status_exit_notify_error_is_not_clean() {
    let status = ShutdownStatus {
        shutdown_request: Ok(()),
        exit_notify: Err("write error".to_string()),
        process_exited: true,
    };
    assert!(!status.is_clean());
}

#[test]
fn shutdown_status_process_not_exited_is_not_clean() {
    let status = ShutdownStatus {
        shutdown_request: Ok(()),
        exit_notify: Ok(()),
        process_exited: false,
    };
    assert!(!status.is_clean());
}

#[test]
fn shutdown_status_all_failed_is_not_clean() {
    let status = ShutdownStatus {
        shutdown_request: Err("a".to_string()),
        exit_notify: Err("b".to_string()),
        process_exited: false,
    };
    assert!(!status.is_clean());
}

// ============================================================================
// Thread ID extraction patterns (production hardening)
// ============================================================================

#[test]
fn thread_id_nested_under_thread_key() {
    // Real codex response: thread ID is at result.thread.id
    let line =
        r#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_abc123","status":"active"}}}"#;
    let msg = ServerMessage::parse(line).unwrap();
    match msg {
        ServerMessage::Response(resp) => {
            let result = resp.result.unwrap();
            let thread_id = result
                .get("thread")
                .and_then(|t| t.get("id"))
                .and_then(|v| v.as_str());
            assert_eq!(thread_id, Some("thr_abc123"));
        }
        _ => panic!("Expected Response"),
    }
}

#[test]
fn thread_id_fallback_to_direct_id() {
    // Fallback: thread ID directly at result.id
    let line = r#"{"jsonrpc":"2.0","id":2,"result":{"id":"thr_direct","status":"active"}}"#;
    let msg = ServerMessage::parse(line).unwrap();
    match msg {
        ServerMessage::Response(resp) => {
            let result = resp.result.unwrap();
            let thread_id = result
                .get("thread")
                .and_then(|t| t.get("id"))
                .or_else(|| result.get("id"))
                .and_then(|v| v.as_str());
            assert_eq!(thread_id, Some("thr_direct"));
        }
        _ => panic!("Expected Response"),
    }
}

#[test]
fn thread_id_prefers_nested_over_direct() {
    // If both exist, nested thread.id should win
    let line = r#"{"jsonrpc":"2.0","id":2,"result":{"id":"wrong","thread":{"id":"thr_correct"}}}"#;
    let msg = ServerMessage::parse(line).unwrap();
    match msg {
        ServerMessage::Response(resp) => {
            let result = resp.result.unwrap();
            let thread_id = result
                .get("thread")
                .and_then(|t| t.get("id"))
                .or_else(|| result.get("id"))
                .and_then(|v| v.as_str());
            assert_eq!(thread_id, Some("thr_correct"));
        }
        _ => panic!("Expected Response"),
    }
}

// ============================================================================
// Turn status classification (production hardening)
// ============================================================================

#[test]
fn turn_completed_status_completed() {
    let line = r#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"t1","status":"completed"}}}"#;
    let msg = ServerMessage::parse(line).unwrap();
    match msg {
        ServerMessage::Notification { params, .. } => {
            let status = params["turn"]["status"].as_str().unwrap();
            assert_eq!(status, "completed");
        }
        _ => panic!("Expected Notification"),
    }
}

#[test]
fn turn_completed_status_interrupted() {
    let line = r#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"t2","status":"interrupted"}}}"#;
    let msg = ServerMessage::parse(line).unwrap();
    match msg {
        ServerMessage::Notification { params, .. } => {
            let status = params["turn"]["status"].as_str().unwrap();
            assert_eq!(status, "interrupted");
        }
        _ => panic!("Expected Notification"),
    }
}

#[test]
fn turn_completed_status_failed_with_error_message() {
    let line = r#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"t3","status":"failed","error":{"message":"Rate limit exceeded"}}}}"#;
    let msg = ServerMessage::parse(line).unwrap();
    match msg {
        ServerMessage::Notification { params, .. } => {
            let status = params["turn"]["status"].as_str().unwrap();
            assert_eq!(status, "failed");
            let err_msg = params["turn"]["error"]["message"].as_str().unwrap();
            assert_eq!(err_msg, "Rate limit exceeded");
        }
        _ => panic!("Expected Notification"),
    }
}

#[test]
fn turn_completed_error_without_message_uses_object_string() {
    // Error object may not have a "message" field
    let line = r#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"t4","status":"failed","error":{"code":500}}}}"#;
    let msg = ServerMessage::parse(line).unwrap();
    match msg {
        ServerMessage::Notification { params, .. } => {
            let err = &params["turn"]["error"];
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| err.to_string());
            assert!(msg.contains("500"));
        }
        _ => panic!("Expected Notification"),
    }
}

// ============================================================================
// Turn ID correlation (production hardening)
// ============================================================================

#[test]
fn turn_id_extraction_from_turn_start_response() {
    // turn/start returns turn ID in result.id or result.turn.id
    let line = r#"{"jsonrpc":"2.0","id":3,"result":{"id":"turn_abc","status":"in_progress"}}"#;
    let msg = ServerMessage::parse(line).unwrap();
    match msg {
        ServerMessage::Response(resp) => {
            let result = resp.result.unwrap();
            let turn_id = result
                .get("id")
                .or_else(|| result.get("turn").and_then(|t| t.get("id")))
                .and_then(|v| v.as_str());
            assert_eq!(turn_id, Some("turn_abc"));
        }
        _ => panic!("Expected Response"),
    }
}

#[test]
fn turn_id_correlation_matching_completion() {
    let expected_turn_id = "turn_xyz";
    let line = r#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"turn_xyz","status":"completed"}}}"#;
    let msg = ServerMessage::parse(line).unwrap();
    match msg {
        ServerMessage::Notification { params, .. } => {
            let actual_id = params["turn"]["id"].as_str().unwrap_or("");
            assert_eq!(actual_id, expected_turn_id);
        }
        _ => panic!("Expected Notification"),
    }
}

#[test]
fn turn_id_correlation_stale_completion_discarded() {
    let expected_turn_id = "turn_new";
    let stale_line = r#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"turn_old","status":"completed"}}}"#;
    let matching_line = r#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"turn_new","status":"completed"}}}"#;

    // Simulate: first completion is stale, second matches
    let completions = [stale_line, matching_line];
    let mut matched = None;

    for line in &completions {
        let msg = ServerMessage::parse(line).unwrap();
        if let ServerMessage::Notification { params, .. } = msg {
            let actual_id = params["turn"]["id"].as_str().unwrap_or("");
            if actual_id == expected_turn_id {
                matched = Some(params);
                break;
            }
            // Otherwise: stale, discard and continue
        }
    }

    assert!(matched.is_some());
    assert_eq!(matched.unwrap()["turn"]["id"], "turn_new");
}

// ============================================================================
// Schema completeness — production requirements
// ============================================================================

#[test]
fn output_schema_findings_items_required_includes_line() {
    let schema = review_output_schema();
    let items_required = schema["properties"]["findings"]["items"]["required"]
        .as_array()
        .unwrap();
    let req_strs: Vec<&str> = items_required.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(
        req_strs.contains(&"line"),
        "findings items 'required' must include 'line' for OpenAI structured output"
    );
}

#[test]
fn output_schema_findings_items_disallows_additional_properties() {
    let schema = review_output_schema();
    let items = &schema["properties"]["findings"]["items"];
    assert_eq!(
        items["additionalProperties"], false,
        "findings items must have additionalProperties: false"
    );
}

#[test]
fn output_schema_has_all_finding_fields() {
    let schema = review_output_schema();
    let props = &schema["properties"]["findings"]["items"]["properties"];
    let expected_fields = [
        "severity",
        "dimension",
        "title",
        "file",
        "line",
        "problem",
        "suggestion",
    ];
    for field in &expected_fields {
        assert!(
            props.get(field).is_some(),
            "findings items schema must have property '{field}'"
        );
    }
}

#[test]
fn output_schema_findings_items_required_has_all_fields() {
    let schema = review_output_schema();
    let items_required = schema["properties"]["findings"]["items"]["required"]
        .as_array()
        .unwrap();
    let req_strs: Vec<&str> = items_required.iter().map(|v| v.as_str().unwrap()).collect();
    let expected = [
        "severity",
        "dimension",
        "title",
        "file",
        "line",
        "problem",
        "suggestion",
    ];
    for field in &expected {
        assert!(
            req_strs.contains(field),
            "findings items 'required' must include '{field}'"
        );
    }
}

// ============================================================================
// Multi-JSON-object stream simulation (production hardening)
// ============================================================================

#[test]
fn simulate_multi_object_stream_last_is_review() {
    // Codex with outputSchema streams reasoning as separate JSON objects
    // then the final structured answer.
    let reasoning = r#"{"thinking":"analyzing code patterns..."}"#;
    let final_answer = r#"{"findings":[],"score":9,"summary":"clean code","strengths":["well structured"]}"#;

    let mut agent_text = String::new();
    agent_text.push_str(reasoning);
    agent_text.push_str(final_answer);

    // Extract all top-level JSON objects
    let mut objects = Vec::new();
    let mut depth = 0i32;
    let mut start = None;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, ch) in agent_text.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        objects.push(&agent_text[s..=i]);
                    }
                    start = None;
                }
            }
            _ => {}
        }
    }

    assert_eq!(objects.len(), 2);

    // Last valid ReviewOutput wins
    let mut review = None;
    for obj in objects.iter().rev() {
        if let Ok(r) = serde_json::from_str::<ReviewOutput>(obj) {
            review = Some(r);
            break;
        }
    }

    let review = review.expect("Should find a valid ReviewOutput");
    assert_eq!(review.score, 9);
    assert_eq!(review.summary, "clean code");
}

#[test]
fn simulate_stream_with_braces_in_strings() {
    // Ensure brace matching handles strings with braces
    let text = r#"{"findings":[],"score":8,"summary":"code uses {} syntax","strengths":["handles {braces}"]}"#;

    let mut depth = 0i32;
    let mut start = None;
    let mut objects = Vec::new();
    let mut in_string = false;
    let mut escape_next = false;

    for (i, ch) in text.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        objects.push(&text[s..=i]);
                    }
                    start = None;
                }
            }
            _ => {}
        }
    }

    // Should be ONE object, not broken by the braces in strings
    assert_eq!(objects.len(), 1);
    let review: ReviewOutput = serde_json::from_str(objects[0]).unwrap();
    assert_eq!(review.score, 8);
    assert!(review.summary.contains("{}"));
}

// ============================================================================
// Agent text accumulation edge cases (production hardening)
// ============================================================================

#[test]
fn agent_text_accumulation_large_delta() {
    // Verify that large deltas work correctly in protocol parsing
    let large_text = "x".repeat(10_000);
    let line = format!(
        r#"{{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{{"delta":"{}"}}}}"#,
        large_text
    );
    let msg = ServerMessage::parse(&line).unwrap();
    match msg {
        ServerMessage::Notification { params, .. } => {
            let delta = params["delta"].as_str().unwrap();
            assert_eq!(delta.len(), 10_000);
        }
        _ => panic!("Expected Notification"),
    }
}

#[test]
fn agent_text_accumulation_unicode_delta() {
    let line = r#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"delta":"한글 테스트 🎉"}}"#;
    let msg = ServerMessage::parse(&line).unwrap();
    match msg {
        ServerMessage::Notification { params, .. } => {
            let delta = params["delta"].as_str().unwrap();
            assert!(delta.contains("한글"));
            assert!(delta.contains("🎉"));
        }
        _ => panic!("Expected Notification"),
    }
}

// ============================================================================
// JsonRpcError — Display with data
// ============================================================================

#[test]
fn json_rpc_error_display_without_data() {
    let err = JsonRpcError {
        code: -32600,
        message: "Invalid request".to_string(),
        data: None,
    };
    let display = format!("{err}");
    assert_eq!(display, "JSON-RPC error -32600: Invalid request");
}

#[test]
fn json_rpc_error_display_with_data() {
    let err_json =
        r#"{"code":-32000,"message":"Internal error","data":{"detail":"stack overflow"}}"#;
    let err: JsonRpcError = serde_json::from_str(err_json).unwrap();
    // Display should only show code + message
    assert_eq!(format!("{err}"), "JSON-RPC error -32000: Internal error");
    // But data should be preserved
    assert!(err.data.is_some());
    assert_eq!(err.data.unwrap()["detail"], "stack overflow");
}

// ============================================================================
// Response with both result and error (ambiguous — should handle gracefully)
// ============================================================================

#[test]
fn parse_response_with_both_result_and_error() {
    // Per JSON-RPC 2.0 spec, only one should be present, but we should handle gracefully
    let line = r#"{"jsonrpc":"2.0","id":7,"result":{"data":"ok"},"error":{"code":-1,"message":"partial"}}"#;
    let msg = ServerMessage::parse(line).unwrap();
    match msg {
        ServerMessage::Response(resp) => {
            assert_eq!(resp.id, Some(7));
            // Both should be preserved — caller decides which to use
            assert!(resp.result.is_some());
            assert!(resp.error.is_some());
        }
        _ => panic!("Expected Response"),
    }
}

// ============================================================================
// Simulate full review with turn ID correlation (production hardening)
// ============================================================================

#[test]
fn simulate_review_with_turn_correlation() {
    // Full flow with turn ID tracking
    let messages = vec![
        r#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":"codex-app-server/0.2.0"}}"#,
        r#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_001","status":"active"}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"result":{"id":"turn_001","status":"in_progress"}}"#,
        r#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"delta":"{\"findings\":[],\"score\":10,\"summary\":\"perfect\",\"strengths\":[\"all good\"]}"}}"#,
        // Stale completion from a previous turn
        r#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"turn_000","status":"completed"}}}"#,
        // Matching completion
        r#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"turn_001","status":"completed"}}}"#,
    ];

    let mut agent_text = String::new();
    let mut thread_id = String::new();
    let mut turn_id = String::new();
    let mut matching_completion = None;
    let expected_turn_id = "turn_001";

    for line in &messages {
        let msg = ServerMessage::parse(line).unwrap();
        match msg {
            ServerMessage::Response(resp) => {
                if let Some(result) = resp.result {
                    // Thread ID extraction (nested preferred)
                    if let Some(tid) = result
                        .get("thread")
                        .and_then(|t| t.get("id"))
                        .and_then(|v| v.as_str())
                    {
                        thread_id = tid.to_string();
                    }
                    // Turn ID extraction
                    if let Some(tid) = result
                        .get("id")
                        .or_else(|| result.get("turn").and_then(|t| t.get("id")))
                        .and_then(|v| v.as_str())
                    {
                        if tid.starts_with("turn_") {
                            turn_id = tid.to_string();
                        }
                    }
                }
            }
            ServerMessage::Notification { method, params } => match method.as_str() {
                "item/agentMessage/delta" => {
                    if let Some(delta) = params.get("delta").and_then(|d| d.as_str()) {
                        agent_text.push_str(delta);
                    }
                }
                "turn/completed" => {
                    let actual_id = params["turn"]["id"].as_str().unwrap_or("");
                    if actual_id == expected_turn_id {
                        matching_completion = Some(params);
                    }
                    // else: stale, discard
                }
                _ => {}
            },
        }
    }

    assert_eq!(thread_id, "thr_001");
    assert_eq!(turn_id, "turn_001");
    assert!(matching_completion.is_some());

    // Parse review from agent text
    let review: ReviewOutput = serde_json::from_str(&agent_text).unwrap();
    assert_eq!(review.score, 10);

    // Verify the matched completion is correct
    let comp = matching_completion.unwrap();
    assert_eq!(comp["turn"]["status"], "completed");
}

// ============================================================================
// CoderOutput — deserialization
// ============================================================================

#[test]
fn coder_output_deserializes_completed() {
    let json_str = r#"{
        "status": "completed",
        "summary": "Implemented rate limiting",
        "files_changed": [
            {"path": "src/middleware/rate-limit.ts", "action": "created", "description": "Rate limiting middleware"},
            {"path": "src/auth/login.ts", "action": "modified", "description": "Applied rate limiter"}
        ],
        "notes": []
    }"#;
    let output: CoderOutput = serde_json::from_str(json_str).unwrap();
    assert_eq!(output.status, CoderStatus::Completed);
    assert_eq!(output.files_changed.len(), 2);
    assert_eq!(output.files_changed[0].action, FileAction::Created);
    assert_eq!(output.files_changed[1].action, FileAction::Modified);
    assert!(output.notes.is_empty());
}

#[test]
fn coder_output_deserializes_partial() {
    let json_str = r#"{
        "status": "partial",
        "summary": "Created middleware but could not modify login",
        "files_changed": [
            {"path": "src/middleware/rate-limit.ts", "action": "created", "description": "Rate limiting middleware"}
        ],
        "notes": ["Could not find login handler entry point"]
    }"#;
    let output: CoderOutput = serde_json::from_str(json_str).unwrap();
    assert_eq!(output.status, CoderStatus::Partial);
    assert_eq!(output.files_changed.len(), 1);
    assert_eq!(output.notes.len(), 1);
}

#[test]
fn coder_output_deserializes_blocked() {
    let json_str = r#"{
        "status": "blocked",
        "summary": "Missing dependency",
        "files_changed": [],
        "notes": ["express-rate-limit package not installed"]
    }"#;
    let output: CoderOutput = serde_json::from_str(json_str).unwrap();
    assert_eq!(output.status, CoderStatus::Blocked);
    assert!(output.files_changed.is_empty());
    assert_eq!(output.notes.len(), 1);
}

#[test]
fn coder_output_deserializes_deleted_action() {
    let json_str = r#"{
        "status": "completed",
        "summary": "Removed deprecated module",
        "files_changed": [
            {"path": "src/old-module.ts", "action": "deleted", "description": "Removed deprecated module"}
        ],
        "notes": []
    }"#;
    let output: CoderOutput = serde_json::from_str(json_str).unwrap();
    assert_eq!(output.files_changed[0].action, FileAction::Deleted);
}

#[test]
fn coder_output_rejects_invalid_status() {
    let json_str = r#"{
        "status": "unknown",
        "summary": "s",
        "files_changed": [],
        "notes": []
    }"#;
    let result = serde_json::from_str::<CoderOutput>(json_str);
    assert!(result.is_err());
}

#[test]
fn coder_output_rejects_invalid_action() {
    let json_str = r#"{
        "status": "completed",
        "summary": "s",
        "files_changed": [{"path": "f", "action": "renamed", "description": "d"}],
        "notes": []
    }"#;
    let result = serde_json::from_str::<CoderOutput>(json_str);
    assert!(result.is_err());
}

#[test]
fn coder_output_rejects_missing_required_fields() {
    let json_str = r#"{"status": "completed", "summary": "s"}"#;
    let result = serde_json::from_str::<CoderOutput>(json_str);
    assert!(result.is_err());
}

#[test]
fn coder_output_rejects_extra_fields() {
    let json_str = r#"{
        "status": "completed",
        "summary": "s",
        "files_changed": [],
        "notes": [],
        "extra": 1
    }"#;
    let result = serde_json::from_str::<CoderOutput>(json_str);
    assert!(result.is_err(), "CoderOutput must reject unknown fields");
}

#[test]
fn file_change_rejects_extra_fields() {
    let json_str = r#"{
        "status": "completed",
        "summary": "s",
        "files_changed": [{"path": "f", "action": "created", "description": "d", "extra": true}],
        "notes": []
    }"#;
    let result = serde_json::from_str::<CoderOutput>(json_str);
    assert!(result.is_err(), "FileChange must reject unknown fields");
}

#[test]
fn coder_output_serializes_roundtrip() {
    let output = CoderOutput {
        status: CoderStatus::Completed,
        summary: "done".to_string(),
        files_changed: vec![FileChange {
            path: "src/main.ts".to_string(),
            action: FileAction::Modified,
            description: "Updated entry point".to_string(),
        }],
        notes: vec!["All good".to_string()],
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: CoderOutput = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.status, CoderStatus::Completed);
    assert_eq!(parsed.files_changed.len(), 1);
    assert_eq!(parsed.files_changed[0].path, "src/main.ts");
}

// ============================================================================
// CoderStatus — Display
// ============================================================================

#[test]
fn coder_status_display() {
    assert_eq!(format!("{}", CoderStatus::Completed), "completed");
    assert_eq!(format!("{}", CoderStatus::Partial), "partial");
    assert_eq!(format!("{}", CoderStatus::Blocked), "blocked");
}

// ============================================================================
// FileAction — Display
// ============================================================================

#[test]
fn file_action_display() {
    assert_eq!(format!("{}", FileAction::Created), "created");
    assert_eq!(format!("{}", FileAction::Modified), "modified");
    assert_eq!(format!("{}", FileAction::Deleted), "deleted");
}

// ============================================================================
// coder_output_schema — schema validation
// ============================================================================

#[test]
fn coder_schema_has_required_fields() {
    let schema = coder_output_schema();
    assert_eq!(schema["type"], "object");

    let required = schema["required"].as_array().unwrap();
    let req_strs: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(req_strs.contains(&"status"));
    assert!(req_strs.contains(&"summary"));
    assert!(req_strs.contains(&"files_changed"));
    assert!(req_strs.contains(&"notes"));
}

#[test]
fn coder_schema_status_has_enum() {
    let schema = coder_output_schema();
    let status = &schema["properties"]["status"];
    assert_eq!(status["type"], "string");
    let enums = status["enum"].as_array().unwrap();
    assert_eq!(enums.len(), 3);
    assert!(enums.contains(&json!("completed")));
    assert!(enums.contains(&json!("partial")));
    assert!(enums.contains(&json!("blocked")));
}

#[test]
fn coder_schema_files_changed_item_has_action_enum() {
    let schema = coder_output_schema();
    let action = &schema["properties"]["files_changed"]["items"]["properties"]["action"];
    assert_eq!(action["type"], "string");
    let enums = action["enum"].as_array().unwrap();
    assert_eq!(enums.len(), 3);
    assert!(enums.contains(&json!("created")));
    assert!(enums.contains(&json!("modified")));
    assert!(enums.contains(&json!("deleted")));
}

#[test]
fn coder_schema_disallows_additional_properties() {
    let schema = coder_output_schema();
    assert_eq!(schema["additionalProperties"], false);
}

#[test]
fn coder_schema_files_changed_items_disallows_additional_properties() {
    let schema = coder_output_schema();
    let items = &schema["properties"]["files_changed"]["items"];
    assert_eq!(items["additionalProperties"], false);
}

#[test]
fn coder_schema_files_changed_items_required_has_all_fields() {
    let schema = coder_output_schema();
    let items_required = schema["properties"]["files_changed"]["items"]["required"]
        .as_array()
        .unwrap();
    let req_strs: Vec<&str> = items_required.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(req_strs.contains(&"path"));
    assert!(req_strs.contains(&"action"));
    assert!(req_strs.contains(&"description"));
}

#[test]
fn coder_schema_has_all_file_change_fields() {
    let schema = coder_output_schema();
    let props = &schema["properties"]["files_changed"]["items"]["properties"];
    for field in &["path", "action", "description"] {
        assert!(
            props.get(field).is_some(),
            "files_changed items schema must have property '{field}'"
        );
    }
}

// ============================================================================
// Simulate full coder stream (production hardening)
// ============================================================================

#[test]
fn simulate_full_coder_stream() {
    let messages = vec![
        // Response to initialize
        r#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":"codex-app-server/0.1.0"}}"#,
        // Response to thread/start (workspace-write)
        r#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_coder_1","status":"active"}}}"#,
        // Response to turn/start
        r#"{"jsonrpc":"2.0","id":3,"result":{"id":"turn_c1","status":"in_progress"}}"#,
        // Streaming deltas
        r#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"delta":"{\"status\":\"completed\""}}"#,
        r#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"delta":",\"summary\":\"done\",\"files_changed\":[{\"path\":\"a.ts\",\"action\":\"created\",\"description\":\"new file\"}]"}}"#,
        r#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"delta":",\"notes\":[]}"}}"#,
        // turn/completed
        r#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"turn_c1","status":"completed"}}}"#,
    ];

    let mut agent_text = String::new();
    let mut turn_completed = false;
    let mut thread_id = String::new();

    for line in &messages {
        let msg = ServerMessage::parse(line).unwrap();
        match msg {
            ServerMessage::Response(resp) => {
                if let Some(result) = resp.result {
                    if let Some(tid) = result
                        .get("thread")
                        .and_then(|t| t.get("id"))
                        .and_then(|v| v.as_str())
                    {
                        thread_id = tid.to_string();
                    }
                }
            }
            ServerMessage::Notification { method, params } => match method.as_str() {
                "item/agentMessage/delta" => {
                    if let Some(delta) = params.get("delta").and_then(|d| d.as_str()) {
                        agent_text.push_str(delta);
                    }
                }
                "turn/completed" => {
                    turn_completed = true;
                }
                _ => {}
            },
        }
    }

    assert_eq!(thread_id, "thr_coder_1");
    assert!(turn_completed);

    // Parse the accumulated text as CoderOutput
    let output: CoderOutput = serde_json::from_str(&agent_text).unwrap();
    assert_eq!(output.status, CoderStatus::Completed);
    assert_eq!(output.files_changed.len(), 1);
    assert_eq!(output.files_changed[0].path, "a.ts");
    assert_eq!(output.summary, "done");
}

// ============================================================================
// Multi-JSON-object stream simulation for coder (production hardening)
// ============================================================================

#[test]
fn simulate_coder_multi_object_stream_last_is_output() {
    let reasoning = r#"{"thinking":"reading files and planning..."}"#;
    let final_answer = r#"{"status":"completed","summary":"implemented feature","files_changed":[{"path":"src/new.ts","action":"created","description":"new module"}],"notes":[]}"#;

    let mut agent_text = String::new();
    agent_text.push_str(reasoning);
    agent_text.push_str(final_answer);

    // Extract all top-level JSON objects
    let mut objects = Vec::new();
    let mut depth = 0i32;
    let mut start = None;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, ch) in agent_text.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        objects.push(&agent_text[s..=i]);
                    }
                    start = None;
                }
            }
            _ => {}
        }
    }

    assert_eq!(objects.len(), 2);

    // Last valid CoderOutput wins
    let mut output = None;
    for obj in objects.iter().rev() {
        if let Ok(r) = serde_json::from_str::<CoderOutput>(obj) {
            output = Some(r);
            break;
        }
    }

    let output = output.expect("Should find a valid CoderOutput");
    assert_eq!(output.status, CoderStatus::Completed);
    assert_eq!(output.summary, "implemented feature");
}
