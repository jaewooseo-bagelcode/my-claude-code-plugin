//! E2E integration tests for aiproxy-common SSE parsing modules.
//! Tests all 3 providers (OpenAI, Anthropic, Gemini) with realistic SSE data.

use aiproxy_common::sse::{SseParser, SseEvent, StreamAction, StopReason};
use aiproxy_common::sse::openai::{parse_openai_responses_sse, parse_openai_chat_sse};
use aiproxy_common::sse::anthropic::parse_anthropic_sse;
use aiproxy_common::sse::gemini::{parse_gemini_sse, GeminiSseState};

// ============================================================================
// SseParser — byte-to-event parsing
// ============================================================================

#[test]
fn sse_parser_basic_event() {
    let mut parser = SseParser::new();
    let events = parser.feed(b"event: message\ndata: {\"hello\":\"world\"}\n\n");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "message");
    assert_eq!(events[0].data, "{\"hello\":\"world\"}");
}

#[test]
fn sse_parser_multiple_events_in_one_chunk() {
    let mut parser = SseParser::new();
    let chunk = b"event: a\ndata: 1\n\nevent: b\ndata: 2\n\n";
    let events = parser.feed(chunk);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, "a");
    assert_eq!(events[1].event_type, "b");
}

#[test]
fn sse_parser_split_across_chunks() {
    let mut parser = SseParser::new();

    let events1 = parser.feed(b"event: text\nda");
    assert!(events1.is_empty(), "Incomplete event should not emit");

    let events2 = parser.feed(b"ta: partial\n\n");
    assert_eq!(events2.len(), 1);
    assert_eq!(events2[0].event_type, "text");
    assert_eq!(events2[0].data, "partial");
}

#[test]
fn sse_parser_handles_crlf() {
    let mut parser = SseParser::new();
    let events = parser.feed(b"event: test\r\ndata: ok\r\n\r\n");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "ok");
}

#[test]
fn sse_parser_flush_handles_missing_trailing_newline() {
    let mut parser = SseParser::new();
    let events = parser.feed(b"event: last\ndata: final");
    assert!(events.is_empty(), "No double newline = no event yet");

    let flushed = parser.flush();
    assert_eq!(flushed.len(), 1);
    assert_eq!(flushed[0].event_type, "last");
    assert_eq!(flushed[0].data, "final");
}

#[test]
fn sse_parser_ignores_empty_events() {
    let mut parser = SseParser::new();
    let events = parser.feed(b"\n\n");
    assert!(events.is_empty(), "Empty double newline should produce no events");
}

#[test]
fn sse_parser_data_without_space() {
    let mut parser = SseParser::new();
    let events = parser.feed(b"event: test\ndata:nospace\n\n");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "nospace");
}

// ============================================================================
// OpenAI Responses API SSE
// ============================================================================

#[test]
fn openai_responses_text_delta() {
    let event = SseEvent {
        event_type: "response.output_text.delta".to_string(),
        data: r#"{"output_index":0,"delta":"Hello"}"#.to_string(),
    };
    let action = parse_openai_responses_sse(&event).unwrap();
    match action {
        StreamAction::TextDelta { index, text } => {
            assert_eq!(index, 0);
            assert_eq!(text, "Hello");
        }
        _ => panic!("Expected TextDelta, got {:?}", action),
    }
}

#[test]
fn openai_responses_function_call_start() {
    let event = SseEvent {
        event_type: "response.output_item.added".to_string(),
        data: r#"{"output_index":1,"item":{"type":"function_call","call_id":"call_abc","name":"Glob"}}"#.to_string(),
    };
    let action = parse_openai_responses_sse(&event).unwrap();
    match action {
        StreamAction::ToolUseStart { index, id, name, .. } => {
            assert_eq!(index, 1);
            assert_eq!(id, "call_abc");
            assert_eq!(name, "Glob");
        }
        _ => panic!("Expected ToolUseStart, got {:?}", action),
    }
}

#[test]
fn openai_responses_function_call_args_delta() {
    let event = SseEvent {
        event_type: "response.function_call_arguments.delta".to_string(),
        data: r#"{"output_index":1,"delta":"{\"pattern\":"}"#.to_string(),
    };
    let action = parse_openai_responses_sse(&event).unwrap();
    match action {
        StreamAction::InputJsonDelta { index, partial_json } => {
            assert_eq!(index, 1);
            assert!(partial_json.contains("pattern"));
        }
        _ => panic!("Expected InputJsonDelta, got {:?}", action),
    }
}

#[test]
fn openai_responses_function_call_args_done() {
    let event = SseEvent {
        event_type: "response.function_call_arguments.done".to_string(),
        data: r#"{"output_index":1,"arguments":"{\"pattern\":\"**/*.rs\"}"}"#.to_string(),
    };
    let action = parse_openai_responses_sse(&event).unwrap();
    match action {
        StreamAction::InputJsonFinal { index, json } => {
            assert_eq!(index, 1);
            assert!(json.contains("**/*.rs"));
        }
        _ => panic!("Expected InputJsonFinal, got {:?}", action),
    }
}

#[test]
fn openai_responses_completed_with_tool_calls() {
    let event = SseEvent {
        event_type: "response.completed".to_string(),
        data: r#"{"response":{"status":"completed","output":[{"type":"function_call","call_id":"call_1","name":"Glob"}]}}"#.to_string(),
    };
    let action = parse_openai_responses_sse(&event).unwrap();
    match action {
        StreamAction::MessageComplete { stop_reason } => {
            assert_eq!(stop_reason, StopReason::ToolUse);
        }
        _ => panic!("Expected MessageComplete, got {:?}", action),
    }
}

#[test]
fn openai_responses_completed_without_tool_calls() {
    let event = SseEvent {
        event_type: "response.completed".to_string(),
        data: r#"{"response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"Done"}]}]}}"#.to_string(),
    };
    let action = parse_openai_responses_sse(&event).unwrap();
    match action {
        StreamAction::MessageComplete { stop_reason } => {
            assert_eq!(stop_reason, StopReason::EndTurn);
        }
        _ => panic!("Expected MessageComplete, got {:?}", action),
    }
}

#[test]
fn openai_responses_error() {
    let event = SseEvent {
        event_type: "error".to_string(),
        data: r#"{"error":{"message":"Rate limit exceeded"}}"#.to_string(),
    };
    let action = parse_openai_responses_sse(&event).unwrap();
    match action {
        StreamAction::Error(msg) => {
            assert!(msg.contains("Rate limit"));
        }
        _ => panic!("Expected Error, got {:?}", action),
    }
}

#[test]
fn openai_responses_max_tokens() {
    let event = SseEvent {
        event_type: "response.completed".to_string(),
        data: r#"{"response":{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[]}}"#.to_string(),
    };
    let action = parse_openai_responses_sse(&event).unwrap();
    match action {
        StreamAction::MessageComplete { stop_reason } => {
            assert_eq!(stop_reason, StopReason::MaxTokens);
        }
        _ => panic!("Expected MessageComplete, got {:?}", action),
    }
}

#[test]
fn openai_responses_unknown_event_ignored() {
    let event = SseEvent {
        event_type: "response.created".to_string(),
        data: r#"{"some":"data"}"#.to_string(),
    };
    assert!(parse_openai_responses_sse(&event).is_none());
}

// ============================================================================
// OpenAI Chat Completions SSE
// ============================================================================

#[test]
fn openai_chat_text_delta() {
    let event = SseEvent {
        event_type: String::new(),
        data: r#"{"choices":[{"delta":{"content":"Hello"},"index":0}]}"#.to_string(),
    };
    let action = parse_openai_chat_sse(&event).unwrap();
    match action {
        StreamAction::TextDelta { text, .. } => {
            assert_eq!(text, "Hello");
        }
        _ => panic!("Expected TextDelta, got {:?}", action),
    }
}

#[test]
fn openai_chat_done_signal() {
    let event = SseEvent {
        event_type: String::new(),
        data: "[DONE]".to_string(),
    };
    let action = parse_openai_chat_sse(&event).unwrap();
    match action {
        StreamAction::MessageComplete { stop_reason } => {
            assert_eq!(stop_reason, StopReason::EndTurn);
        }
        _ => panic!("Expected MessageComplete, got {:?}", action),
    }
}

#[test]
fn openai_chat_finish_reason_stop() {
    let event = SseEvent {
        event_type: String::new(),
        data: r#"{"choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#.to_string(),
    };
    let action = parse_openai_chat_sse(&event).unwrap();
    match action {
        StreamAction::MessageComplete { stop_reason } => {
            assert_eq!(stop_reason, StopReason::EndTurn);
        }
        _ => panic!("Expected MessageComplete, got {:?}", action),
    }
}

#[test]
fn openai_chat_finish_reason_length() {
    let event = SseEvent {
        event_type: String::new(),
        data: r#"{"choices":[{"delta":{},"finish_reason":"length","index":0}]}"#.to_string(),
    };
    let action = parse_openai_chat_sse(&event).unwrap();
    match action {
        StreamAction::MessageComplete { stop_reason } => {
            assert_eq!(stop_reason, StopReason::MaxTokens);
        }
        _ => panic!("Expected MessageComplete, got {:?}", action),
    }
}

#[test]
fn openai_chat_error() {
    let event = SseEvent {
        event_type: String::new(),
        data: r#"{"error":{"message":"Server overloaded"}}"#.to_string(),
    };
    let action = parse_openai_chat_sse(&event).unwrap();
    match action {
        StreamAction::Error(msg) => {
            assert!(msg.contains("Server overloaded"));
        }
        _ => panic!("Expected Error, got {:?}", action),
    }
}

// ============================================================================
// Anthropic Messages API SSE
// ============================================================================

#[test]
fn anthropic_text_delta() {
    let event = SseEvent {
        event_type: "content_block_delta".to_string(),
        data: r#"{"index":0,"delta":{"type":"text_delta","text":"Hello world"}}"#.to_string(),
    };
    let action = parse_anthropic_sse(&event).unwrap();
    match action {
        StreamAction::TextDelta { index, text } => {
            assert_eq!(index, 0);
            assert_eq!(text, "Hello world");
        }
        _ => panic!("Expected TextDelta, got {:?}", action),
    }
}

#[test]
fn anthropic_tool_use_start() {
    let event = SseEvent {
        event_type: "content_block_start".to_string(),
        data: r#"{"index":1,"content_block":{"type":"tool_use","id":"toolu_123","name":"read_file"}}"#.to_string(),
    };
    let action = parse_anthropic_sse(&event).unwrap();
    match action {
        StreamAction::ToolUseStart { index, id, name, .. } => {
            assert_eq!(index, 1);
            assert_eq!(id, "toolu_123");
            assert_eq!(name, "read_file");
        }
        _ => panic!("Expected ToolUseStart, got {:?}", action),
    }
}

#[test]
fn anthropic_input_json_delta() {
    let event = SseEvent {
        event_type: "content_block_delta".to_string(),
        data: r#"{"index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"src/"}}"#.to_string(),
    };
    let action = parse_anthropic_sse(&event).unwrap();
    match action {
        StreamAction::InputJsonDelta { index, partial_json } => {
            assert_eq!(index, 1);
            assert!(partial_json.contains("path"));
        }
        _ => panic!("Expected InputJsonDelta, got {:?}", action),
    }
}

#[test]
fn anthropic_content_block_stop() {
    let event = SseEvent {
        event_type: "content_block_stop".to_string(),
        data: r#"{"index":0}"#.to_string(),
    };
    let action = parse_anthropic_sse(&event).unwrap();
    match action {
        StreamAction::ContentBlockStop { index } => {
            assert_eq!(index, 0);
        }
        _ => panic!("Expected ContentBlockStop, got {:?}", action),
    }
}

#[test]
fn anthropic_message_delta_end_turn() {
    let event = SseEvent {
        event_type: "message_delta".to_string(),
        data: r#"{"delta":{"stop_reason":"end_turn"}}"#.to_string(),
    };
    let action = parse_anthropic_sse(&event).unwrap();
    match action {
        StreamAction::MessageComplete { stop_reason } => {
            assert_eq!(stop_reason, StopReason::EndTurn);
        }
        _ => panic!("Expected MessageComplete, got {:?}", action),
    }
}

#[test]
fn anthropic_message_delta_tool_use() {
    let event = SseEvent {
        event_type: "message_delta".to_string(),
        data: r#"{"delta":{"stop_reason":"tool_use"}}"#.to_string(),
    };
    let action = parse_anthropic_sse(&event).unwrap();
    match action {
        StreamAction::MessageComplete { stop_reason } => {
            assert_eq!(stop_reason, StopReason::ToolUse);
        }
        _ => panic!("Expected MessageComplete with ToolUse, got {:?}", action),
    }
}

#[test]
fn anthropic_message_delta_max_tokens() {
    let event = SseEvent {
        event_type: "message_delta".to_string(),
        data: r#"{"delta":{"stop_reason":"max_tokens"}}"#.to_string(),
    };
    let action = parse_anthropic_sse(&event).unwrap();
    match action {
        StreamAction::MessageComplete { stop_reason } => {
            assert_eq!(stop_reason, StopReason::MaxTokens);
        }
        _ => panic!("Expected MessageComplete with MaxTokens, got {:?}", action),
    }
}

#[test]
fn anthropic_ping() {
    let event = SseEvent {
        event_type: "ping".to_string(),
        data: "{}".to_string(),
    };
    let action = parse_anthropic_sse(&event).unwrap();
    matches!(action, StreamAction::Ping);
}

#[test]
fn anthropic_error() {
    let event = SseEvent {
        event_type: "error".to_string(),
        data: r#"{"error":{"message":"Overloaded"}}"#.to_string(),
    };
    let action = parse_anthropic_sse(&event).unwrap();
    match action {
        StreamAction::Error(msg) => {
            assert!(msg.contains("Overloaded"));
        }
        _ => panic!("Expected Error, got {:?}", action),
    }
}

#[test]
fn anthropic_message_start_ignored() {
    let event = SseEvent {
        event_type: "message_start".to_string(),
        data: r#"{"message":{"id":"msg_123"}}"#.to_string(),
    };
    assert!(parse_anthropic_sse(&event).is_none());
}

// ============================================================================
// Gemini streamGenerateContent SSE
// ============================================================================

#[test]
fn gemini_text_delta() {
    let mut state = GeminiSseState::new();
    let event = SseEvent {
        event_type: String::new(),
        data: r#"{"candidates":[{"content":{"parts":[{"text":"Hello from Gemini"}]}}]}"#.to_string(),
    };
    let actions = parse_gemini_sse(&event, &mut state);
    assert_eq!(actions.len(), 1);
    match &actions[0] {
        StreamAction::TextDelta { text, .. } => {
            assert_eq!(text, "Hello from Gemini");
        }
        _ => panic!("Expected TextDelta, got {:?}", actions[0]),
    }
}

#[test]
fn gemini_incremental_text_across_chunks() {
    let mut state = GeminiSseState::new();

    let event1 = SseEvent {
        event_type: String::new(),
        data: r#"{"candidates":[{"content":{"parts":[{"text":"First "}]}}]}"#.to_string(),
    };
    let actions1 = parse_gemini_sse(&event1, &mut state);
    assert_eq!(actions1.len(), 1);

    let event2 = SseEvent {
        event_type: String::new(),
        data: r#"{"candidates":[{"content":{"parts":[{"text":"Second"}]}}]}"#.to_string(),
    };
    let actions2 = parse_gemini_sse(&event2, &mut state);
    assert_eq!(actions2.len(), 1);
    match &actions2[0] {
        StreamAction::TextDelta { text, .. } => {
            assert_eq!(text, "Second");
        }
        _ => panic!("Expected TextDelta"),
    }
}

#[test]
fn gemini_function_call() {
    let mut state = GeminiSseState::new();
    let event = SseEvent {
        event_type: String::new(),
        data: r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"Glob","args":{"pattern":"**/*.rs"}}}]}}]}"#.to_string(),
    };
    let actions = parse_gemini_sse(&event, &mut state);
    assert!(actions.len() >= 2, "Should have ToolUseStart + InputJsonDelta");

    match &actions[0] {
        StreamAction::ToolUseStart { name, .. } => {
            assert_eq!(name, "Glob");
        }
        _ => panic!("Expected ToolUseStart, got {:?}", actions[0]),
    }
    match &actions[1] {
        StreamAction::InputJsonDelta { partial_json, .. } => {
            assert!(partial_json.contains("**/*.rs"));
        }
        _ => panic!("Expected InputJsonDelta, got {:?}", actions[1]),
    }
}

#[test]
fn gemini_function_call_with_thought_signature() {
    let mut state = GeminiSseState::new();
    let event = SseEvent {
        event_type: String::new(),
        data: r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"Read","args":{"path":"lib.rs"}},"thoughtSignature":"sig_abc123"}]}}]}"#.to_string(),
    };
    let actions = parse_gemini_sse(&event, &mut state);

    match &actions[0] {
        StreamAction::ToolUseStart { name, thought_signature, .. } => {
            assert_eq!(name, "Read");
            assert_eq!(thought_signature.as_deref(), Some("sig_abc123"));
        }
        _ => panic!("Expected ToolUseStart with thoughtSignature"),
    }
    // Verify state tracks the signature
    assert!(!state.thought_signatures.is_empty());
}

#[test]
fn gemini_stop_reason_end_turn() {
    let mut state = GeminiSseState::new();

    // First send some text to populate state
    let event1 = SseEvent {
        event_type: String::new(),
        data: r#"{"candidates":[{"content":{"parts":[{"text":"Done"}]}}]}"#.to_string(),
    };
    parse_gemini_sse(&event1, &mut state);

    // Now send finish
    let event2 = SseEvent {
        event_type: String::new(),
        data: r#"{"candidates":[{"content":{"parts":[{"text":""}]},"finishReason":"STOP"}]}"#.to_string(),
    };
    let actions = parse_gemini_sse(&event2, &mut state);

    let has_complete = actions.iter().any(|a| matches!(a, StreamAction::MessageComplete { stop_reason: StopReason::EndTurn }));
    assert!(has_complete, "Should have MessageComplete with EndTurn");
}

#[test]
fn gemini_stop_reason_tool_use() {
    let mut state = GeminiSseState::new();

    // Send function call
    let event1 = SseEvent {
        event_type: String::new(),
        data: r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"Glob","args":{"pattern":"*.rs"}}}]}}]}"#.to_string(),
    };
    parse_gemini_sse(&event1, &mut state);

    // Finish with STOP — but since there are tool calls, stop reason should be ToolUse
    let event2 = SseEvent {
        event_type: String::new(),
        data: r#"{"candidates":[{"content":{"parts":[]},"finishReason":"STOP"}]}"#.to_string(),
    };
    let actions = parse_gemini_sse(&event2, &mut state);

    let has_tool_use = actions.iter().any(|a| matches!(a, StreamAction::MessageComplete { stop_reason: StopReason::ToolUse }));
    assert!(has_tool_use, "Should detect ToolUse when function_calls present: {:?}", actions);
}

#[test]
fn gemini_error() {
    let mut state = GeminiSseState::new();
    let event = SseEvent {
        event_type: String::new(),
        data: r#"{"error":{"message":"Quota exceeded"}}"#.to_string(),
    };
    let actions = parse_gemini_sse(&event, &mut state);
    assert_eq!(actions.len(), 1);
    match &actions[0] {
        StreamAction::Error(msg) => {
            assert!(msg.contains("Quota exceeded"));
        }
        _ => panic!("Expected Error, got {:?}", actions[0]),
    }
}

#[test]
fn gemini_mixed_text_and_function_call() {
    let mut state = GeminiSseState::new();
    let event = SseEvent {
        event_type: String::new(),
        data: r#"{"candidates":[{"content":{"parts":[{"text":"Let me check"},{"functionCall":{"name":"Grep","args":{"query":"TODO"}}}]}}]}"#.to_string(),
    };
    let actions = parse_gemini_sse(&event, &mut state);

    // Should have TextDelta + ToolUseStart + InputJsonDelta
    let has_text = actions.iter().any(|a| matches!(a, StreamAction::TextDelta { .. }));
    let has_tool = actions.iter().any(|a| matches!(a, StreamAction::ToolUseStart { .. }));
    assert!(has_text, "Should have text delta");
    assert!(has_tool, "Should have tool use start");
}

#[test]
fn gemini_no_candidates_ignored() {
    let mut state = GeminiSseState::new();
    let event = SseEvent {
        event_type: String::new(),
        data: r#"{"modelVersion":"gemini-3-pro"}"#.to_string(),
    };
    let actions = parse_gemini_sse(&event, &mut state);
    assert!(actions.is_empty(), "No candidates should produce no actions");
}

// ============================================================================
// Full SSE pipeline: parser + provider parser combined
// ============================================================================

#[test]
fn full_pipeline_openai_responses() {
    let mut parser = SseParser::new();
    let raw = concat!(
        "event: response.output_text.delta\n",
        "data: {\"output_index\":0,\"delta\":\"Hello\"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"output_index\":0,\"delta\":\" world\"}\n\n",
        "event: response.completed\n",
        "data: {\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"message\"}]}}\n\n",
    );

    let events = parser.feed(raw.as_bytes());
    assert_eq!(events.len(), 3);

    let mut text = String::new();
    let mut completed = false;

    for event in &events {
        if let Some(action) = parse_openai_responses_sse(event) {
            match action {
                StreamAction::TextDelta { text: t, .. } => text.push_str(&t),
                StreamAction::MessageComplete { .. } => completed = true,
                _ => {}
            }
        }
    }

    assert_eq!(text, "Hello world");
    assert!(completed);
}

#[test]
fn full_pipeline_anthropic() {
    let mut parser = SseParser::new();
    let raw = concat!(
        "event: content_block_delta\n",
        "data: {\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Review \"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"complete\"}}\n\n",
        "event: message_delta\n",
        "data: {\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
    );

    let events = parser.feed(raw.as_bytes());
    assert_eq!(events.len(), 3);

    let mut text = String::new();
    let mut stop = None;

    for event in &events {
        if let Some(action) = parse_anthropic_sse(event) {
            match action {
                StreamAction::TextDelta { text: t, .. } => text.push_str(&t),
                StreamAction::MessageComplete { stop_reason } => stop = Some(stop_reason),
                _ => {}
            }
        }
    }

    assert_eq!(text, "Review complete");
    assert_eq!(stop, Some(StopReason::EndTurn));
}

#[test]
fn full_pipeline_gemini() {
    let mut parser = SseParser::new();
    let mut state = GeminiSseState::new();

    let raw = concat!(
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Analyzing\"}]}}]}\n\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" code\"}]}}]}\n\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[]},\"finishReason\":\"STOP\"}]}\n\n",
    );

    let events = parser.feed(raw.as_bytes());
    assert_eq!(events.len(), 3);

    let mut text = String::new();
    let mut completed = false;

    for event in &events {
        for action in parse_gemini_sse(event, &mut state) {
            match action {
                StreamAction::TextDelta { text: t, .. } => text.push_str(&t),
                StreamAction::MessageComplete { .. } => completed = true,
                _ => {}
            }
        }
    }

    assert_eq!(text, "Analyzing code");
    assert!(completed);
}
