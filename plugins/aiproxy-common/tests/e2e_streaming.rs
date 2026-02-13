//! E2E tests for aiproxy-common streaming accumulator.
//! Tests StreamAction → StreamResult conversion for all 4 providers (no HTTP).

use aiproxy_common::sse::{StreamAction, StopReason};
use aiproxy_common::sse::streaming::StreamAccumulator;

// ============================================================================
// StreamAccumulator — basic accumulation
// ============================================================================

#[test]
fn accumulator_text_only() {
    let mut acc = StreamAccumulator::new();
    acc.process(StreamAction::TextDelta { index: 0, text: "Hello ".into() });
    acc.process(StreamAction::TextDelta { index: 0, text: "world".into() });
    acc.process(StreamAction::MessageComplete { stop_reason: StopReason::EndTurn });

    let result = acc.into_result();
    assert_eq!(result.text, "Hello world");
    assert!(result.tool_calls.is_empty());
    assert_eq!(result.stop_reason, StopReason::EndTurn);
    assert!(result.response_id.is_none());
}

#[test]
fn accumulator_single_tool_call() {
    let mut acc = StreamAccumulator::new();
    acc.process(StreamAction::ToolUseStart {
        index: 0,
        id: "call_1".into(),
        name: "Glob".into(),
        thought_signature: None,
    });
    acc.process(StreamAction::InputJsonDelta {
        index: 0,
        partial_json: r#"{"pattern":""#.into(),
    });
    acc.process(StreamAction::InputJsonDelta {
        index: 0,
        partial_json: r#"**/*.rs"}"#.into(),
    });
    acc.process(StreamAction::ContentBlockStop { index: 0 });
    acc.process(StreamAction::MessageComplete { stop_reason: StopReason::ToolUse });

    let result = acc.into_result();
    assert!(result.text.is_empty());
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0].id, "call_1");
    assert_eq!(result.tool_calls[0].name, "Glob");
    assert_eq!(result.tool_calls[0].arguments, r#"{"pattern":"**/*.rs"}"#);
    assert_eq!(result.stop_reason, StopReason::ToolUse);
}

#[test]
fn accumulator_text_plus_tool_call() {
    let mut acc = StreamAccumulator::new();
    // Text first
    acc.process(StreamAction::TextDelta { index: 0, text: "Let me check the files.".into() });
    acc.process(StreamAction::ContentBlockStop { index: 0 });
    // Then tool
    acc.process(StreamAction::ToolUseStart {
        index: 1,
        id: "call_abc".into(),
        name: "Read".into(),
        thought_signature: None,
    });
    acc.process(StreamAction::InputJsonDelta {
        index: 1,
        partial_json: r#"{"path":"main.rs"}"#.into(),
    });
    acc.process(StreamAction::ContentBlockStop { index: 1 });
    acc.process(StreamAction::MessageComplete { stop_reason: StopReason::ToolUse });

    let result = acc.into_result();
    assert_eq!(result.text, "Let me check the files.");
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0].name, "Read");
    assert_eq!(result.stop_reason, StopReason::ToolUse);
}

#[test]
fn accumulator_multiple_tool_calls() {
    let mut acc = StreamAccumulator::new();
    acc.process(StreamAction::ToolUseStart {
        index: 0,
        id: "call_1".into(),
        name: "Glob".into(),
        thought_signature: None,
    });
    acc.process(StreamAction::InputJsonDelta {
        index: 0,
        partial_json: r#"{"pattern":"*.rs"}"#.into(),
    });
    acc.process(StreamAction::ToolUseStart {
        index: 1,
        id: "call_2".into(),
        name: "Grep".into(),
        thought_signature: None,
    });
    acc.process(StreamAction::InputJsonDelta {
        index: 1,
        partial_json: r#"{"query":"TODO"}"#.into(),
    });
    acc.process(StreamAction::MessageComplete { stop_reason: StopReason::ToolUse });

    let result = acc.into_result();
    assert_eq!(result.tool_calls.len(), 2);
    assert_eq!(result.tool_calls[0].name, "Glob");
    assert_eq!(result.tool_calls[1].name, "Grep");
}

// ============================================================================
// OpenAI Responses API sequences
// ============================================================================

#[test]
fn openai_responses_text_and_complete() {
    let mut acc = StreamAccumulator::new();
    acc.response_id = Some("resp_abc123".into());

    acc.process(StreamAction::TextDelta { index: 0, text: "Review ".into() });
    acc.process(StreamAction::TextDelta { index: 0, text: "findings:".into() });
    acc.process(StreamAction::MessageComplete { stop_reason: StopReason::EndTurn });

    let result = acc.into_result();
    assert_eq!(result.response_id.as_deref(), Some("resp_abc123"));
    assert_eq!(result.text, "Review findings:");
    assert_eq!(result.stop_reason, StopReason::EndTurn);
}

#[test]
fn openai_responses_tool_call_with_final_json() {
    let mut acc = StreamAccumulator::new();
    acc.response_id = Some("resp_xyz".into());

    acc.process(StreamAction::ToolUseStart {
        index: 1,
        id: "call_abc".into(),
        name: "Glob".into(),
        thought_signature: None,
    });
    // Delta comes first
    acc.process(StreamAction::InputJsonDelta {
        index: 1,
        partial_json: r#"{"patt"#.into(),
    });
    acc.process(StreamAction::InputJsonDelta {
        index: 1,
        partial_json: r#"ern":"**/*.rs"}"#.into(),
    });
    // Final replaces delta buffer
    acc.process(StreamAction::InputJsonFinal {
        index: 1,
        json: r#"{"pattern":"**/*.rs"}"#.into(),
    });
    acc.process(StreamAction::MessageComplete { stop_reason: StopReason::ToolUse });

    let result = acc.into_result();
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0].arguments, r#"{"pattern":"**/*.rs"}"#);
    assert_eq!(result.response_id.as_deref(), Some("resp_xyz"));
    assert_eq!(result.stop_reason, StopReason::ToolUse);
}

#[test]
fn openai_responses_max_tokens() {
    let mut acc = StreamAccumulator::new();
    acc.process(StreamAction::TextDelta { index: 0, text: "Partial output".into() });
    acc.process(StreamAction::MessageComplete { stop_reason: StopReason::MaxTokens });

    let result = acc.into_result();
    assert_eq!(result.text, "Partial output");
    assert_eq!(result.stop_reason, StopReason::MaxTokens);
}

// ============================================================================
// OpenAI Chat Completions sequences
// ============================================================================

#[test]
fn openai_chat_text_stream() {
    let mut acc = StreamAccumulator::new();
    acc.process(StreamAction::TextDelta { index: 0, text: "Summary: ".into() });
    acc.process(StreamAction::TextDelta { index: 0, text: "All good.".into() });
    acc.process(StreamAction::MessageComplete { stop_reason: StopReason::EndTurn });

    let result = acc.into_result();
    assert_eq!(result.text, "Summary: All good.");
    assert!(result.tool_calls.is_empty());
    assert_eq!(result.stop_reason, StopReason::EndTurn);
    assert!(result.response_id.is_none());
}

// ============================================================================
// Anthropic Messages API sequences
// ============================================================================

#[test]
fn anthropic_multi_block_text_and_tool() {
    let mut acc = StreamAccumulator::new();
    // Text block
    acc.process(StreamAction::TextDelta { index: 0, text: "I'll read the file.".into() });
    acc.process(StreamAction::ContentBlockStop { index: 0 });
    // Tool use block
    acc.process(StreamAction::ToolUseStart {
        index: 1,
        id: "toolu_123".into(),
        name: "read_file".into(),
        thought_signature: None,
    });
    acc.process(StreamAction::InputJsonDelta {
        index: 1,
        partial_json: r#"{"path":"#.into(),
    });
    acc.process(StreamAction::InputJsonDelta {
        index: 1,
        partial_json: r#""src/main.rs"}"#.into(),
    });
    acc.process(StreamAction::ContentBlockStop { index: 1 });
    acc.process(StreamAction::MessageComplete { stop_reason: StopReason::ToolUse });

    let result = acc.into_result();
    assert_eq!(result.text, "I'll read the file.");
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0].id, "toolu_123");
    assert_eq!(result.tool_calls[0].name, "read_file");
    assert_eq!(result.tool_calls[0].arguments, r#"{"path":"src/main.rs"}"#);
    assert_eq!(result.stop_reason, StopReason::ToolUse);
}

#[test]
fn anthropic_text_only_end_turn() {
    let mut acc = StreamAccumulator::new();
    acc.process(StreamAction::TextDelta { index: 0, text: "Code looks good!".into() });
    acc.process(StreamAction::ContentBlockStop { index: 0 });
    acc.process(StreamAction::MessageComplete { stop_reason: StopReason::EndTurn });

    let result = acc.into_result();
    assert_eq!(result.text, "Code looks good!");
    assert!(result.tool_calls.is_empty());
    assert_eq!(result.stop_reason, StopReason::EndTurn);
}

#[test]
fn anthropic_multiple_tool_calls() {
    let mut acc = StreamAccumulator::new();
    // Two consecutive tool_use blocks
    acc.process(StreamAction::ToolUseStart {
        index: 0,
        id: "toolu_a".into(),
        name: "Glob".into(),
        thought_signature: None,
    });
    acc.process(StreamAction::InputJsonDelta {
        index: 0,
        partial_json: r#"{"pattern":"**/*.rs"}"#.into(),
    });
    acc.process(StreamAction::ContentBlockStop { index: 0 });
    acc.process(StreamAction::ToolUseStart {
        index: 1,
        id: "toolu_b".into(),
        name: "Read".into(),
        thought_signature: None,
    });
    acc.process(StreamAction::InputJsonDelta {
        index: 1,
        partial_json: r#"{"path":"lib.rs"}"#.into(),
    });
    acc.process(StreamAction::ContentBlockStop { index: 1 });
    acc.process(StreamAction::MessageComplete { stop_reason: StopReason::ToolUse });

    let result = acc.into_result();
    assert_eq!(result.tool_calls.len(), 2);
    assert_eq!(result.tool_calls[0].id, "toolu_a");
    assert_eq!(result.tool_calls[1].id, "toolu_b");
}

// ============================================================================
// Gemini streamGenerateContent sequences
// ============================================================================

#[test]
fn gemini_text_and_function_call() {
    let mut acc = StreamAccumulator::new();
    // Text part
    acc.process(StreamAction::TextDelta { index: 0, text: "Let me check".into() });
    // Function call with thoughtSignature
    acc.process(StreamAction::ToolUseStart {
        index: 1,
        id: "uuid-123".into(),
        name: "Grep".into(),
        thought_signature: Some("sig_abc".into()),
    });
    acc.process(StreamAction::InputJsonDelta {
        index: 1,
        partial_json: r#"{"query":"TODO"}"#.into(),
    });
    acc.process(StreamAction::ContentBlockStop { index: 0 });
    acc.process(StreamAction::ContentBlockStop { index: 1 });
    acc.process(StreamAction::MessageComplete { stop_reason: StopReason::ToolUse });

    let result = acc.into_result();
    assert_eq!(result.text, "Let me check");
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0].name, "Grep");
    assert_eq!(result.tool_calls[0].thought_signature.as_deref(), Some("sig_abc"));
    assert_eq!(result.stop_reason, StopReason::ToolUse);
}

#[test]
fn gemini_text_only_stop() {
    let mut acc = StreamAccumulator::new();
    acc.process(StreamAction::TextDelta { index: 0, text: "Analysis ".into() });
    acc.process(StreamAction::TextDelta { index: 0, text: "complete.".into() });
    acc.process(StreamAction::ContentBlockStop { index: 0 });
    acc.process(StreamAction::MessageComplete { stop_reason: StopReason::EndTurn });

    let result = acc.into_result();
    assert_eq!(result.text, "Analysis complete.");
    assert!(result.tool_calls.is_empty());
    assert_eq!(result.stop_reason, StopReason::EndTurn);
}

#[test]
fn gemini_multiple_function_calls() {
    let mut acc = StreamAccumulator::new();
    acc.process(StreamAction::ToolUseStart {
        index: 0,
        id: "uuid-1".into(),
        name: "Glob".into(),
        thought_signature: Some("sig_1".into()),
    });
    acc.process(StreamAction::InputJsonDelta {
        index: 0,
        partial_json: r#"{"pattern":"*.rs"}"#.into(),
    });
    acc.process(StreamAction::ToolUseStart {
        index: 1,
        id: "uuid-2".into(),
        name: "Read".into(),
        thought_signature: Some("sig_2".into()),
    });
    acc.process(StreamAction::InputJsonDelta {
        index: 1,
        partial_json: r#"{"path":"lib.rs"}"#.into(),
    });
    acc.process(StreamAction::ContentBlockStop { index: 0 });
    acc.process(StreamAction::ContentBlockStop { index: 1 });
    acc.process(StreamAction::MessageComplete { stop_reason: StopReason::ToolUse });

    let result = acc.into_result();
    assert_eq!(result.tool_calls.len(), 2);
    assert_eq!(result.tool_calls[0].thought_signature.as_deref(), Some("sig_1"));
    assert_eq!(result.tool_calls[1].thought_signature.as_deref(), Some("sig_2"));
}

// ============================================================================
// Edge cases
// ============================================================================

#[test]
fn accumulator_error_with_no_content() {
    let mut acc = StreamAccumulator::new();
    acc.process(StreamAction::Error("Server overloaded".into()));

    let result = acc.into_result();
    assert!(result.text.contains("SSE Error"));
    assert!(result.text.contains("Server overloaded"));
    assert_eq!(result.stop_reason, StopReason::Unknown);
}

#[test]
fn accumulator_error_with_existing_content() {
    let mut acc = StreamAccumulator::new();
    acc.process(StreamAction::TextDelta { index: 0, text: "Partial content".into() });
    acc.process(StreamAction::Error("Connection reset".into()));

    let result = acc.into_result();
    // Existing content is preserved, error text is NOT appended
    assert_eq!(result.text, "Partial content");
    assert_eq!(result.stop_reason, StopReason::Unknown);
}

#[test]
fn accumulator_ping_ignored() {
    let mut acc = StreamAccumulator::new();
    acc.process(StreamAction::Ping);
    acc.process(StreamAction::TextDelta { index: 0, text: "Hello".into() });
    acc.process(StreamAction::Ping);
    acc.process(StreamAction::MessageComplete { stop_reason: StopReason::EndTurn });

    let result = acc.into_result();
    assert_eq!(result.text, "Hello");
    assert_eq!(result.stop_reason, StopReason::EndTurn);
}

#[test]
fn accumulator_input_json_final_replaces_deltas() {
    let mut acc = StreamAccumulator::new();
    acc.process(StreamAction::ToolUseStart {
        index: 0,
        id: "call_1".into(),
        name: "Glob".into(),
        thought_signature: None,
    });
    // Deltas build up partial JSON
    acc.process(StreamAction::InputJsonDelta {
        index: 0,
        partial_json: r#"{"patt"#.into(),
    });
    acc.process(StreamAction::InputJsonDelta {
        index: 0,
        partial_json: r#"ern":"*.ts"#.into(),
    });
    // Final replaces everything
    acc.process(StreamAction::InputJsonFinal {
        index: 0,
        json: r#"{"pattern":"**/*.ts"}"#.into(),
    });
    acc.process(StreamAction::MessageComplete { stop_reason: StopReason::ToolUse });

    let result = acc.into_result();
    assert_eq!(result.tool_calls[0].arguments, r#"{"pattern":"**/*.ts"}"#);
}

#[test]
fn accumulator_empty_stream() {
    let acc = StreamAccumulator::new();
    let result = acc.into_result();
    assert!(result.text.is_empty());
    assert!(result.tool_calls.is_empty());
    assert_eq!(result.stop_reason, StopReason::Unknown);
    assert!(result.response_id.is_none());
}

// ============================================================================
// stop_reason fallback (into_result inference)
// ============================================================================

#[test]
fn fallback_text_without_message_complete_infers_end_turn() {
    let mut acc = StreamAccumulator::new();
    // Stream ended without MessageComplete (e.g. Ok(None) before completion event)
    acc.process(StreamAction::TextDelta { index: 0, text: "Partial review".into() });
    // No MessageComplete — stop_reason stays Unknown

    let result = acc.into_result();
    assert_eq!(result.text, "Partial review");
    assert_eq!(result.stop_reason, StopReason::EndTurn, "Text without completion → EndTurn");
}

#[test]
fn fallback_tool_calls_without_message_complete_infers_tool_use() {
    let mut acc = StreamAccumulator::new();
    acc.process(StreamAction::ToolUseStart {
        index: 0,
        id: "call_x".into(),
        name: "Glob".into(),
        thought_signature: None,
    });
    acc.process(StreamAction::InputJsonDelta {
        index: 0,
        partial_json: r#"{"pattern":"*.rs"}"#.into(),
    });
    // No MessageComplete

    let result = acc.into_result();
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.stop_reason, StopReason::ToolUse, "Tool calls without completion → ToolUse");
}

#[test]
fn fallback_text_and_tool_calls_without_message_complete_infers_tool_use() {
    let mut acc = StreamAccumulator::new();
    acc.process(StreamAction::TextDelta { index: 0, text: "Checking...".into() });
    acc.process(StreamAction::ToolUseStart {
        index: 1,
        id: "call_y".into(),
        name: "Read".into(),
        thought_signature: None,
    });
    acc.process(StreamAction::InputJsonDelta {
        index: 1,
        partial_json: r#"{"path":"lib.rs"}"#.into(),
    });
    // No MessageComplete

    let result = acc.into_result();
    assert_eq!(result.stop_reason, StopReason::ToolUse, "Tool calls take priority over text");
}

#[test]
fn fallback_does_not_apply_on_error() {
    let mut acc = StreamAccumulator::new();
    acc.process(StreamAction::Error("Server error".into()));
    // Error sets text to "[SSE Error] ..." but fallback should NOT promote to EndTurn

    let result = acc.into_result();
    assert_eq!(result.stop_reason, StopReason::Unknown, "Error → stays Unknown");
}

// ============================================================================
// Full pipeline: SseParser + provider parser + accumulator
// ============================================================================

use aiproxy_common::sse::SseParser;
use aiproxy_common::sse::openai::parse_openai_responses_sse;

#[test]
fn full_pipeline_openai_responses_to_stream_result() {
    let mut parser = SseParser::new();
    let mut acc = StreamAccumulator::new();

    let raw = concat!(
        "event: response.created\n",
        "data: {\"response\":{\"id\":\"resp_test123\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"output_index\":0,\"delta\":\"Found \"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"output_index\":0,\"delta\":\"issues.\"}\n\n",
        "event: response.completed\n",
        "data: {\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"message\"}]}}\n\n",
    );

    for event in parser.feed(raw.as_bytes()) {
        // Extract response_id from response.created
        if event.event_type == "response.created" {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&event.data) {
                if let Some(id) = data.get("response")
                    .and_then(|r| r.get("id"))
                    .and_then(|i| i.as_str())
                {
                    acc.response_id = Some(id.to_string());
                }
            }
        }
        if let Some(action) = parse_openai_responses_sse(&event) {
            acc.process(action);
        }
    }

    let result = acc.into_result();
    assert_eq!(result.response_id.as_deref(), Some("resp_test123"));
    assert_eq!(result.text, "Found issues.");
    assert_eq!(result.stop_reason, StopReason::EndTurn);
}

use aiproxy_common::sse::openai::parse_openai_chat_sse;

#[test]
fn full_pipeline_openai_chat_to_stream_result() {
    let mut parser = SseParser::new();
    let mut acc = StreamAccumulator::new();

    let raw = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hello \"},\"index\":0}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"there.\"},\"index\":0}]}\n\n",
        "data: [DONE]\n\n",
    );

    for event in parser.feed(raw.as_bytes()) {
        if let Some(action) = parse_openai_chat_sse(&event) {
            acc.process(action);
        }
    }

    let result = acc.into_result();
    assert_eq!(result.text, "Hello there.");
    assert_eq!(result.stop_reason, StopReason::EndTurn);
}

use aiproxy_common::sse::anthropic::parse_anthropic_sse;

#[test]
fn full_pipeline_anthropic_to_stream_result() {
    let mut parser = SseParser::new();
    let mut acc = StreamAccumulator::new();

    let raw = concat!(
        "event: content_block_delta\n",
        "data: {\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"I'll check.\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"index\":0}\n\n",
        "event: content_block_start\n",
        "data: {\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_abc\",\"name\":\"Glob\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"pattern\\\":\\\"*.rs\\\"}\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"index\":1}\n\n",
        "event: message_delta\n",
        "data: {\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n",
    );

    for event in parser.feed(raw.as_bytes()) {
        if let Some(action) = parse_anthropic_sse(&event) {
            acc.process(action);
        }
    }

    let result = acc.into_result();
    assert_eq!(result.text, "I'll check.");
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0].id, "toolu_abc");
    assert_eq!(result.tool_calls[0].name, "Glob");
    assert_eq!(result.stop_reason, StopReason::ToolUse);
}

use aiproxy_common::sse::gemini::{parse_gemini_sse, GeminiSseState};

#[test]
fn full_pipeline_gemini_to_stream_result() {
    let mut parser = SseParser::new();
    let mut state = GeminiSseState::new();
    let mut acc = StreamAccumulator::new();

    let raw = concat!(
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Analyzing \"}]}}]}\n\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"code.\"}]}}]}\n\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[]},\"finishReason\":\"STOP\"}]}\n\n",
    );

    for event in parser.feed(raw.as_bytes()) {
        for action in parse_gemini_sse(&event, &mut state) {
            acc.process(action);
        }
    }

    let result = acc.into_result();
    assert_eq!(result.text, "Analyzing code.");
    assert_eq!(result.stop_reason, StopReason::EndTurn);
}

#[test]
fn full_pipeline_gemini_function_call_to_stream_result() {
    let mut parser = SseParser::new();
    let mut state = GeminiSseState::new();
    let mut acc = StreamAccumulator::new();

    let raw = concat!(
        "data: {\"candidates\":[{\"content\":{\"parts\":[",
        "{\"functionCall\":{\"name\":\"Read\",\"args\":{\"path\":\"lib.rs\"}},",
        "\"thoughtSignature\":\"sig_test\"}]}}]}\n\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[]},\"finishReason\":\"STOP\"}]}\n\n",
    );

    for event in parser.feed(raw.as_bytes()) {
        for action in parse_gemini_sse(&event, &mut state) {
            acc.process(action);
        }
    }

    let result = acc.into_result();
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0].name, "Read");
    assert_eq!(result.tool_calls[0].thought_signature.as_deref(), Some("sig_test"));
    assert_eq!(result.stop_reason, StopReason::ToolUse);
}
