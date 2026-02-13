//! E2E integration tests for aiproxy-common session module.
//! Tests JsonlLogger, ParticipantSession, and utility functions.

use aiproxy_common::session::{JsonlLogger, ParticipantSession, now_millis, summarize_args};
use serde_json::{json, Value};
use std::io::Read;

// ============================================================================
// JsonlLogger
// ============================================================================

#[test]
fn logger_writes_jsonl_entries() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("test.jsonl");
    let log_path_str = log_path.to_string_lossy().to_string();

    let logger = JsonlLogger::new(&log_path_str);
    logger.log("test_event", 0, Some(json!({"key": "value"})));
    logger.log("another_event", 1, None);
    logger.close();

    let mut content = String::new();
    std::fs::File::open(&log_path)
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();

    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2, "Should have 2 log entries");

    // Parse first entry
    let entry1: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(entry1["event"], "test_event");
    assert_eq!(entry1["iteration"], 0);
    assert_eq!(entry1["data"]["key"], "value");
    assert!(entry1["ts"].as_u64().unwrap() > 0, "Should have timestamp");

    // Parse second entry
    let entry2: Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(entry2["event"], "another_event");
    assert_eq!(entry2["iteration"], 1);
    assert!(entry2.get("data").is_none() || entry2["data"].is_null(), "data should be absent/null when None");
}

#[test]
fn logger_nil_safe_on_invalid_path() {
    // Logger should not panic on invalid path — just be a no-op
    let logger = JsonlLogger::new("/nonexistent/deeply/nested/path/log.jsonl");
    logger.log("should_not_crash", 0, None);
    logger.close();
    // If we got here without panic, the test passes
}

#[test]
fn logger_appends_to_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("append.jsonl");
    let log_path_str = log_path.to_string_lossy().to_string();

    // First logger
    let logger1 = JsonlLogger::new(&log_path_str);
    logger1.log("first", 0, None);
    logger1.close();

    // Second logger (same file)
    let logger2 = JsonlLogger::new(&log_path_str);
    logger2.log("second", 1, None);
    logger2.close();

    let content = std::fs::read_to_string(&log_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2, "Should have entries from both loggers");
}

#[test]
fn logger_timestamps_are_increasing() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("timestamps.jsonl");
    let log_path_str = log_path.to_string_lossy().to_string();

    let logger = JsonlLogger::new(&log_path_str);
    logger.log("first", 0, None);
    std::thread::sleep(std::time::Duration::from_millis(10));
    logger.log("second", 1, None);
    logger.close();

    let content = std::fs::read_to_string(&log_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    let ts1: u64 = serde_json::from_str::<Value>(lines[0]).unwrap()["ts"].as_u64().unwrap();
    let ts2: u64 = serde_json::from_str::<Value>(lines[1]).unwrap()["ts"].as_u64().unwrap();
    assert!(ts2 >= ts1, "Timestamps should be non-decreasing");
}

// ============================================================================
// ParticipantSession
// ============================================================================

#[test]
fn participant_session_lifecycle() {
    let mut session = ParticipantSession::new("openai", "gpt-5.2-codex");

    assert_eq!(session.provider, "openai");
    assert_eq!(session.model, "gpt-5.2-codex");
    assert!(session.steps.is_empty());
    assert!(!session.success);

    // Add a successful tool call
    session.add_tool_call(
        "Glob",
        json!({"pattern": "**/*.rs"}),
        Ok("found 10 files".to_string()),
    );

    assert_eq!(session.steps.len(), 1);
    assert_eq!(session.steps[0].step, 1);
    assert_eq!(session.steps[0].step_type, "tool_call");
    assert_eq!(session.steps[0].tool_name.as_deref(), Some("Glob"));
    assert!(session.steps[0].tool_output.is_some());
    assert!(session.steps[0].tool_error.is_none());
    assert!(session.steps[0].timestamp > 0);

    // Add a failed tool call
    session.add_tool_call(
        "Read",
        json!({"path": "../etc/passwd"}),
        Err("Access denied".to_string()),
    );

    assert_eq!(session.steps.len(), 2);
    assert_eq!(session.steps[1].step, 2);
    assert!(session.steps[1].tool_output.is_none());
    assert_eq!(session.steps[1].tool_error.as_deref(), Some("Access denied"));

    // Finalize
    session.finalize("Review complete. Found 3 issues.".to_string(), true, None);

    assert!(session.success);
    assert_eq!(session.final_content, "Review complete. Found 3 issues.");
    assert!(session.error.is_none());
}

#[test]
fn participant_session_finalize_with_error() {
    let mut session = ParticipantSession::new("gemini", "gemini-3-pro");
    session.finalize(String::new(), false, Some("API timeout".to_string()));

    assert!(!session.success);
    assert_eq!(session.error.as_deref(), Some("API timeout"));
}

#[test]
fn participant_session_to_ai_response() {
    let mut session = ParticipantSession::new("claude", "claude-opus-4-6");
    session.finalize("Analysis done".to_string(), true, None);

    let response = session.to_ai_response();
    assert_eq!(response.provider, "claude");
    assert_eq!(response.model, "claude-opus-4-6");
    assert_eq!(response.content, "Analysis done");
    assert!(response.success);
    assert!(response.error.is_none());
}

#[test]
fn participant_session_serialization_roundtrip() {
    let mut session = ParticipantSession::new("openai", "gpt-5.2");
    session.add_tool_call("Glob", json!({"pattern": "*.rs"}), Ok("files".to_string()));
    session.finalize("Done".to_string(), true, None);

    let json_str = serde_json::to_string(&session).unwrap();
    let deserialized: ParticipantSession = serde_json::from_str(&json_str).unwrap();

    assert_eq!(deserialized.provider, "openai");
    assert_eq!(deserialized.steps.len(), 1);
    assert!(deserialized.success);
    assert_eq!(deserialized.final_content, "Done");
}

// ============================================================================
// Utility functions
// ============================================================================

#[test]
fn now_millis_returns_reasonable_timestamp() {
    let ts = now_millis();
    // Should be after 2024-01-01 (1704067200000) and before 2030-01-01 (1893456000000)
    assert!(ts > 1704067200000, "Timestamp too small: {}", ts);
    assert!(ts < 1893456000000, "Timestamp too large: {}", ts);
}

#[test]
fn summarize_args_short_string_unchanged() {
    let short = "hello";
    assert_eq!(summarize_args(short, 200), "hello");
}

#[test]
fn summarize_args_truncates_long_string() {
    let long = "a".repeat(300);
    let result = summarize_args(&long, 200);
    assert_eq!(result.len(), 203); // 200 + "..."
    assert!(result.ends_with("..."));
}

#[test]
fn summarize_args_exact_boundary() {
    let exact = "a".repeat(200);
    let result = summarize_args(&exact, 200);
    assert_eq!(result, exact, "Exact length should not be truncated");
}
