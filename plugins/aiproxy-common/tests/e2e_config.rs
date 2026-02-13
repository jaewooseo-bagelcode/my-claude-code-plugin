//! E2E integration tests for aiproxy-common config module.
//! Tests URL routing, auth headers, project memory loading.

use aiproxy_common::config::{AIProxyConfig, load_project_memory, build_http_client};

// ============================================================================
// AIProxyConfig — URL routing (aiproxy mode)
// ============================================================================

fn make_aiproxy_config() -> AIProxyConfig {
    AIProxyConfig {
        base_url: "https://aiproxy.example.com".to_string(),
        token: "aiproxy_test_token".to_string(),
        no_aiproxy: false,
        anthropic_api_key: None,
        openai_api_key: None,
        gemini_api_key: None,
    }
}

fn make_direct_config() -> AIProxyConfig {
    AIProxyConfig {
        base_url: String::new(),
        token: String::new(),
        no_aiproxy: true,
        anthropic_api_key: Some("sk-ant-test".to_string()),
        openai_api_key: Some("sk-openai-test".to_string()),
        gemini_api_key: Some("AIzaSy-test".to_string()),
    }
}

#[test]
fn aiproxy_anthropic_url() {
    let config = make_aiproxy_config();
    let url = config.anthropic_url("/v1/messages");
    assert_eq!(url, "https://aiproxy.example.com/anthropic/v1/messages");
}

#[test]
fn aiproxy_anthropic_auth() {
    let config = make_aiproxy_config();
    let (header, value) = config.anthropic_auth();
    assert_eq!(header, "Authorization");
    assert_eq!(value, "Bearer aiproxy_test_token");
}

#[test]
fn aiproxy_openai_url() {
    let config = make_aiproxy_config();
    let url = config.openai_url("/v1/responses");
    assert_eq!(url, "https://aiproxy.example.com/openai/v1/responses");
}

#[test]
fn aiproxy_openai_token() {
    let config = make_aiproxy_config();
    assert_eq!(config.openai_token(), "aiproxy_test_token");
}

#[test]
fn aiproxy_gemini_url() {
    let config = make_aiproxy_config();
    let url = config.gemini_url("/v1beta/models/gemini-3-pro:generateContent");
    assert_eq!(url, "https://aiproxy.example.com/google-vertex/v1beta/models/gemini-3-pro:generateContent");
}

#[test]
fn aiproxy_gemini_auth() {
    let config = make_aiproxy_config();
    let (header, value) = config.gemini_auth();
    assert_eq!(header, "Authorization");
    assert_eq!(value, "Bearer aiproxy_test_token");
}

#[test]
fn aiproxy_base_url_trailing_slash_stripped() {
    let config = AIProxyConfig {
        base_url: "https://aiproxy.example.com/".to_string(),
        token: "test".to_string(),
        no_aiproxy: false,
        anthropic_api_key: None,
        openai_api_key: None,
        gemini_api_key: None,
    };
    let url = config.openai_url("/v1/responses");
    assert_eq!(url, "https://aiproxy.example.com/openai/v1/responses");
}

// ============================================================================
// AIProxyConfig — URL routing (direct/no_aiproxy mode)
// ============================================================================

#[test]
fn direct_anthropic_url() {
    let config = make_direct_config();
    let url = config.anthropic_url("/v1/messages");
    assert_eq!(url, "https://api.anthropic.com/v1/messages");
}

#[test]
fn direct_anthropic_auth() {
    let config = make_direct_config();
    let (header, value) = config.anthropic_auth();
    assert_eq!(header, "x-api-key");
    assert_eq!(value, "sk-ant-test");
}

#[test]
fn direct_openai_url() {
    let config = make_direct_config();
    let url = config.openai_url("/v1/chat/completions");
    assert_eq!(url, "https://api.openai.com/v1/chat/completions");
}

#[test]
fn direct_openai_token() {
    let config = make_direct_config();
    assert_eq!(config.openai_token(), "sk-openai-test");
}

#[test]
fn direct_gemini_url() {
    let config = make_direct_config();
    let url = config.gemini_url("/v1beta/models/gemini-3-pro:streamGenerateContent");
    assert_eq!(url, "https://generativelanguage.googleapis.com/v1beta/models/gemini-3-pro:streamGenerateContent");
}

#[test]
fn direct_gemini_auth() {
    let config = make_direct_config();
    let (header, value) = config.gemini_auth();
    assert_eq!(header, "x-goog-api-key");
    assert_eq!(value, "AIzaSy-test");
}

// ============================================================================
// HTTP Client
// ============================================================================

#[test]
fn build_http_client_succeeds() {
    let client = build_http_client();
    // Just verify it doesn't panic and returns a valid client
    let _ = client;
}

// ============================================================================
// Project memory loading
// ============================================================================

#[test]
fn load_project_memory_from_real_repo() {
    // Use the workspace root which has CLAUDE.md
    let manifest = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest).parent().unwrap().parent().unwrap();
    let result = load_project_memory(&workspace_root.to_string_lossy());

    assert!(result.is_some(), "Should find CLAUDE.md in repo root");
    let memory = result.unwrap();
    assert!(memory.contains("CLAUDE.md"), "Should contain CLAUDE.md section header");
}

#[test]
fn load_project_memory_empty_for_nonexistent_dir() {
    let result = load_project_memory("/nonexistent/path/that/does/not/exist");
    assert!(result.is_none(), "Should return None for nonexistent path");
}

#[test]
fn load_project_memory_from_tempdir_without_claude_md() {
    let dir = tempfile::tempdir().unwrap();
    let result = load_project_memory(&dir.path().to_string_lossy());
    assert!(result.is_none(), "Empty dir should have no project memory");
}

#[test]
fn load_project_memory_finds_claude_md() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("CLAUDE.md"), "# Project Rules\nBe nice").unwrap();
    let result = load_project_memory(&dir.path().to_string_lossy());
    assert!(result.is_some());
    let memory = result.unwrap();
    assert!(memory.contains("Be nice"));
}

#[test]
fn load_project_memory_finds_rules_dir() {
    let dir = tempfile::tempdir().unwrap();
    let rules_dir = dir.path().join(".claude/rules");
    std::fs::create_dir_all(&rules_dir).unwrap();
    std::fs::write(rules_dir.join("security.md"), "# Security Rules\nNo secrets in code").unwrap();
    std::fs::write(rules_dir.join("style.md"), "# Style Rules\nUse kebab-case").unwrap();

    let result = load_project_memory(&dir.path().to_string_lossy());
    assert!(result.is_some());
    let memory = result.unwrap();
    assert!(memory.contains("No secrets in code"), "Should include security rules");
    assert!(memory.contains("Use kebab-case"), "Should include style rules");
}
