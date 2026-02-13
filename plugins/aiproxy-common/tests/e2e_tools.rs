//! E2E integration tests for aiproxy-common tools module.
//! Runs against the real filesystem and real git repo.

use aiproxy_common::tools;
use serde_json::{json, Value};

/// Helper: get this repo's root (workspace root = plugins/)
fn repo_root() -> String {
    let manifest = env!("CARGO_MANIFEST_DIR"); // .../plugins/aiproxy-common
    std::path::Path::new(manifest)
        .parent()
        .unwrap()
        .to_string_lossy()
        .to_string()
}

// ============================================================================
// Glob tool
// ============================================================================

#[tokio::test]
async fn glob_finds_rust_files_with_doublestar() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Glob",
        json!({"pattern": "**/*.rs"}),
        &root,
    )
    .await
    .expect("Glob should succeed");

    let parsed: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    assert!(parsed["count"].as_u64().unwrap() > 0, "Should find .rs files");

    let results = parsed["results"].as_array().unwrap();
    assert!(
        results.iter().any(|r| r.as_str().unwrap().contains("lib.rs")),
        "Should find lib.rs among results"
    );
}

#[tokio::test]
async fn glob_finds_toml_at_root() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Glob",
        json!({"pattern": "*.toml"}),
        &root,
    )
    .await
    .expect("Glob should succeed");

    let parsed: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);

    let results = parsed["results"].as_array().unwrap();
    assert!(
        results.iter().any(|r| r.as_str().unwrap() == "Cargo.toml"),
        "Should find workspace Cargo.toml"
    );
}

#[tokio::test]
async fn glob_with_path_prefix() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Glob",
        json!({"pattern": "aiproxy-common/src/**/*.rs"}),
        &root,
    )
    .await
    .expect("Glob should succeed");

    let parsed: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    assert!(parsed["count"].as_u64().unwrap() >= 4, "Should find multiple .rs files in aiproxy-common/src/");
}

#[tokio::test]
async fn glob_max_results_limits_output() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Glob",
        json!({"pattern": "**/*.rs", "max_results": 3}),
        &root,
    )
    .await
    .expect("Glob should succeed");

    let parsed: Value = serde_json::from_str(&result).unwrap();
    assert!(parsed["count"].as_u64().unwrap() <= 3, "Should respect max_results");
}

#[tokio::test]
async fn glob_empty_pattern_fails() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Glob",
        json!({"pattern": ""}),
        &root,
    )
    .await;

    assert!(result.is_err(), "Empty pattern should fail");
}

#[tokio::test]
async fn glob_full_naming_works() {
    let root = repo_root();
    let result = tools::execute_tool(
        "glob_files",
        json!({"pattern": "**/*.toml"}),
        &root,
    )
    .await
    .expect("glob_files should work (dual naming)");

    let parsed: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    assert!(parsed["count"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn glob_skips_git_directory() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Glob",
        json!({"pattern": "**/*"}),
        &root,
    )
    .await
    .expect("Glob should succeed");

    let parsed: Value = serde_json::from_str(&result).unwrap();
    let results = parsed["results"].as_array().unwrap();
    for r in results {
        let path = r.as_str().unwrap();
        assert!(!path.starts_with(".git/"), "Should skip .git directory: {}", path);
    }
}

// ============================================================================
// Grep tool
// ============================================================================

#[tokio::test]
async fn grep_files_with_matches_mode() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Grep",
        json!({"query": "pub fn execute"}),
        &root,
    )
    .await
    .expect("Grep should succeed");

    assert!(!result.is_empty(), "Should find matches");
    // files_with_matches is default mode - returns file paths
    assert!(result.contains(".rs"), "Should find .rs files containing 'pub fn execute'");
}

#[tokio::test]
async fn grep_content_mode_with_line_numbers() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Grep",
        json!({"query": "pub mod tools", "output_mode": "content"}),
        &root,
    )
    .await
    .expect("Grep should succeed");

    assert!(!result.is_empty(), "Should find content matches");
    // Content mode shows file:line:content format
    assert!(result.contains("pub mod tools"), "Should include matching line content");
}

#[tokio::test]
async fn grep_count_mode() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Grep",
        json!({"query": "use serde", "output_mode": "count"}),
        &root,
    )
    .await
    .expect("Grep should succeed");

    assert!(!result.is_empty(), "Should find count results");
    // Count mode shows file:count format
    assert!(result.contains(":"), "Should have file:count format");
}

#[tokio::test]
async fn grep_with_glob_filter() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Grep",
        json!({"query": "fn execute", "glob": "**/*.rs"}),
        &root,
    )
    .await
    .expect("Grep should succeed");

    assert!(!result.is_empty(), "Should find matches in .rs files");
    // All results should be .rs files
    for line in result.lines() {
        if !line.is_empty() {
            assert!(line.contains(".rs"), "Result should be .rs file: {}", line);
        }
    }
}

#[tokio::test]
async fn grep_case_insensitive() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Grep",
        json!({"query": "PUB MOD", "case_insensitive": true}),
        &root,
    )
    .await
    .expect("Grep should succeed with case insensitive");

    assert!(!result.is_empty(), "Case insensitive search should find 'pub mod'");
}

#[tokio::test]
async fn grep_with_context_lines() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Grep",
        json!({"query": "pub struct SseParser", "output_mode": "content", "context": 2}),
        &root,
    )
    .await
    .expect("Grep should succeed with context");

    assert!(!result.is_empty(), "Should find SseParser");
    // Context mode adds surrounding lines
    let lines: Vec<&str> = result.lines().collect();
    assert!(lines.len() > 1, "Context should include surrounding lines");
}

#[tokio::test]
async fn grep_head_limit() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Grep",
        json!({"query": "use", "output_mode": "content", "head_limit": 5}),
        &root,
    )
    .await
    .expect("Grep should succeed with head_limit");

    let lines: Vec<&str> = result.lines().collect();
    assert!(lines.len() <= 5, "head_limit should limit output to 5 lines, got {}", lines.len());
}

#[tokio::test]
async fn grep_regex_pattern() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Grep",
        json!({"query": "fn\\s+execute\\w*\\("}),
        &root,
    )
    .await
    .expect("Grep regex should work");

    assert!(!result.is_empty(), "Regex should match execute functions");
}

#[tokio::test]
async fn grep_empty_pattern_fails() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Grep",
        json!({"query": ""}),
        &root,
    )
    .await;

    assert!(result.is_err(), "Empty query should fail");
}

#[tokio::test]
async fn grep_full_naming_with_pattern_key() {
    let root = repo_root();
    // grep_content uses "pattern" key instead of "query"
    let result = tools::execute_tool(
        "grep_content",
        json!({"pattern": "pub fn execute"}),
        &root,
    )
    .await
    .expect("grep_content with pattern key should work (dual naming)");

    assert!(!result.is_empty());
}

// ============================================================================
// Read tool
// ============================================================================

#[tokio::test]
async fn read_file_relative_path() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Read",
        json!({"path": "aiproxy-common/src/lib.rs"}),
        &root,
    )
    .await
    .expect("Read should succeed");

    assert!(result.contains("pub mod"), "Should contain module declarations");
}

#[tokio::test]
async fn read_file_with_offset_and_limit() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Read",
        json!({"path": "aiproxy-common/src/lib.rs", "offset": 1, "limit": 2}),
        &root,
    )
    .await
    .expect("Read with offset/limit should succeed");

    let lines: Vec<&str> = result.lines().collect();
    assert!(lines.len() <= 2, "Should return at most 2 lines, got {}", lines.len());
}

#[tokio::test]
async fn read_file_with_end_line() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Read",
        json!({"path": "aiproxy-common/src/lib.rs", "start_line": 1, "end_line": 2}),
        &root,
    )
    .await
    .expect("Read with start_line/end_line should succeed");

    let lines: Vec<&str> = result.lines().collect();
    assert!(lines.len() <= 2, "Should return lines 1-2");
}

#[tokio::test]
async fn read_file_path_traversal_rejected() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Read",
        json!({"path": "../../../etc/passwd"}),
        &root,
    )
    .await;

    assert!(result.is_err(), "Path traversal should be rejected");
    let err = result.unwrap_err();
    assert!(
        err.contains("..") || err.contains("denied") || err.contains("outside"),
        "Error should mention traversal: {}", err
    );
}

#[tokio::test]
async fn read_file_denied_path() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Read",
        json!({"path": ".git/HEAD"}),
        &root,
    )
    .await;

    assert!(result.is_err(), ".git/HEAD should be denied");
}

#[tokio::test]
async fn read_directory_rejected() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Read",
        json!({"path": "aiproxy-common/src"}),
        &root,
    )
    .await;

    assert!(result.is_err(), "Reading a directory should fail");
}

#[tokio::test]
async fn read_nonexistent_file() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Read",
        json!({"path": "nonexistent_file_xyz.rs"}),
        &root,
    )
    .await;

    assert!(result.is_err(), "Nonexistent file should fail");
}

#[tokio::test]
async fn read_full_naming_works() {
    let root = repo_root();
    let result = tools::execute_tool(
        "read_file",
        json!({"file_path": "aiproxy-common/src/lib.rs"}),
        &root,
    )
    .await
    .expect("read_file with file_path key should work (dual naming)");

    assert!(result.contains("pub mod"));
}

#[tokio::test]
async fn read_line_number_format() {
    let root = repo_root();
    let result = tools::execute_tool(
        "Read",
        json!({"path": "aiproxy-common/src/lib.rs"}),
        &root,
    )
    .await
    .expect("Read should succeed");

    // Verify line number format: "     1|pub mod config"
    let first_line = result.lines().next().unwrap();
    assert!(first_line.contains("|"), "Should have line_number|content format: {}", first_line);
    assert!(first_line.contains("1"), "First line should be line 1");
}

// ============================================================================
// GitDiff tool
// ============================================================================

#[tokio::test]
async fn git_diff_runs_without_error() {
    let root = repo_root();
    let result = tools::execute_tool(
        "GitDiff",
        json!({}),
        &root,
    )
    .await
    .expect("GitDiff should succeed");

    // May be empty if no uncommitted changes, but should not error
    assert!(
        !result.is_empty(),
        "GitDiff should return something (changes or 'no changes')"
    );
}

#[tokio::test]
async fn git_diff_with_base_branch() {
    let root = repo_root();
    let result = tools::execute_tool(
        "GitDiff",
        json!({"base": "main"}),
        &root,
    )
    .await
    .expect("GitDiff with base=main should succeed");

    assert!(!result.is_empty());
}

#[tokio::test]
async fn git_diff_invalid_branch_name_rejected() {
    let root = repo_root();
    let result = tools::execute_tool(
        "GitDiff",
        json!({"base": "'; rm -rf /"}),
        &root,
    )
    .await;

    assert!(result.is_err(), "Invalid branch name should be rejected");
}

#[tokio::test]
async fn git_diff_with_file_path() {
    let root = repo_root();
    let result = tools::execute_tool(
        "GitDiff",
        json!({"path": "Cargo.toml"}),
        &root,
    )
    .await
    .expect("GitDiff with file path should succeed");

    assert!(!result.is_empty());
}

#[tokio::test]
async fn git_diff_full_naming_works() {
    let root = repo_root();
    let result = tools::execute_tool(
        "git_diff",
        json!({}),
        &root,
    )
    .await
    .expect("git_diff should work (dual naming)");

    assert!(!result.is_empty());
}

// ============================================================================
// Unknown tool
// ============================================================================

#[tokio::test]
async fn unknown_tool_returns_error() {
    let root = repo_root();
    let result = tools::execute_tool(
        "NonexistentTool",
        json!({}),
        &root,
    )
    .await;

    assert!(result.is_err(), "Unknown tool should fail");
    assert!(result.unwrap_err().contains("Unknown tool"));
}

// ============================================================================
// Tool definitions
// ============================================================================

#[test]
fn code_review_tool_definitions_have_4_tools() {
    let defs = tools::build_tool_definitions(tools::ToolSet::CodeReview);
    assert_eq!(defs.len(), 4, "CodeReview should have Glob, Grep, Read, GitDiff");

    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"Glob"));
    assert!(names.contains(&"Grep"));
    assert!(names.contains(&"Read"));
    assert!(names.contains(&"GitDiff"));
}

#[test]
fn full_tool_definitions_have_4_tools() {
    let defs = tools::build_tool_definitions(tools::ToolSet::Full);
    assert_eq!(defs.len(), 4, "Full should have glob_files, grep_content, read_file, git_diff");

    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"glob_files"));
    assert!(names.contains(&"grep_content"));
    assert!(names.contains(&"read_file"));
    assert!(names.contains(&"git_diff"));
}

#[test]
fn tool_definitions_have_valid_json_schemas() {
    for tool_set in [tools::ToolSet::CodeReview, tools::ToolSet::Full] {
        let defs = tools::build_tool_definitions(tool_set);
        for def in &defs {
            assert!(!def.name.is_empty(), "Tool name should not be empty");
            assert!(!def.description.is_empty(), "Tool description should not be empty");
            assert!(def.parameters.is_object(), "Parameters should be a JSON object for {}", def.name);
            assert!(def.parameters.get("type").is_some(), "Parameters should have 'type' field for {}", def.name);
        }
    }
}

// ============================================================================
// Deny list
// ============================================================================

#[test]
fn deny_list_blocks_git_and_env() {
    assert!(tools::is_denied_path(".git/HEAD"));
    assert!(tools::is_denied_path("src/.git/config"));
    assert!(tools::is_denied_path(".env"));
    assert!(tools::is_denied_path("node_modules/foo/bar.js"));
    assert!(tools::is_denied_path("foo.pyc"));
    assert!(tools::is_denied_path("bar.exe"));
    assert!(tools::is_denied_path("data.sqlite"));
}

#[test]
fn deny_list_allows_normal_files() {
    assert!(!tools::is_denied_path("src/main.rs"));
    assert!(!tools::is_denied_path("Cargo.toml"));
    assert!(!tools::is_denied_path("README.md"));
    assert!(!tools::is_denied_path("src/config.rs"));
}
