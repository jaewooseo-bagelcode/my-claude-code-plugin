use regex::Regex;
use std::io::{BufRead, BufReader};
use walkdir::WalkDir;

const DEFAULT_MAX_RESULTS: usize = 200;
const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024; // 2MB
const MAX_OUTPUT_BYTES: usize = 200 * 1024;

/// Directories to skip during walk
fn skip_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | "node_modules" | ".venv" | "__pycache__" | ".codex-sessions"
            | "target" | ".svn" | ".hg"
    )
}

/// Compile a glob pattern to regex for file filtering (reuse Go compileGlob logic)
fn compile_glob_filter(pattern: &str) -> Option<Regex> {
    let pattern = pattern.replace('\\', "/");
    let parts: Vec<&str> = pattern.split('/').collect();
    let mut re_parts = Vec::new();

    for part in &parts {
        if *part == "**" {
            re_parts.push("(?:.+/)?".to_string());
        } else {
            re_parts.push(format!("{}/", glob_segment_to_regex(part)));
        }
    }

    let mut re_str = format!("^{}", re_parts.join(""));
    re_str = re_str.replace("(?:.+/)?/", "(?:.+/)?");
    re_str.push('$');

    Regex::new(&re_str).ok()
}

fn glob_segment_to_regex(seg: &str) -> String {
    let mut b = String::new();
    let chars: Vec<char> = seg.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '*' => b.push_str("[^/]*"),
            '?' => b.push_str("[^/]"),
            '[' => {
                if let Some(j) = seg[i..].find(']') {
                    b.push_str(&seg[i..i + j + 1]);
                    i += j;
                } else {
                    b.push_str(&regex::escape(&chars[i].to_string()));
                }
            }
            ch => b.push_str(&regex::escape(&ch.to_string())),
        }
        i += 1;
    }

    b
}

pub fn execute(
    pattern: &str,
    path: &str,
    glob_filter: Option<&str>,
    output_mode: Option<&str>,
    case_insensitive: Option<bool>,
    context: Option<usize>,
    head_limit: Option<usize>,
    max_results: Option<usize>,
) -> Result<String, String> {
    if pattern.is_empty() {
        return Err("Grep: query/pattern required".to_string());
    }

    let mode = output_mode.unwrap_or("files_with_matches");
    let max = max_results.unwrap_or(DEFAULT_MAX_RESULTS).min(DEFAULT_MAX_RESULTS);

    // Compile regex; fallback to literal match on invalid regex (Go style)
    let re = if case_insensitive.unwrap_or(false) {
        Regex::new(&format!("(?i){}", pattern))
            .or_else(|_| Regex::new(&format!("(?i){}", regex::escape(pattern))))
            .map_err(|e| format!("Grep: failed to compile pattern: {}", e))?
    } else {
        Regex::new(pattern)
            .or_else(|_| Regex::new(&regex::escape(pattern)))
            .map_err(|e| format!("Grep: failed to compile pattern: {}", e))?
    };

    // Compile glob filter
    let glob_matcher = glob_filter.and_then(compile_glob_filter);

    let ctx_lines = context.unwrap_or(0);

    let mut matches: Vec<String> = Vec::new();
    let mut files_matched: Vec<String> = Vec::new();
    let mut count_results: Vec<String> = Vec::new();
    let mut total_output_bytes = 0usize;

    let walker = WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.file_type().is_dir() {
                let name = e.file_name().to_str().unwrap_or("");
                return !skip_dir(name);
            }
            true
        });

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        if entry.file_type().is_dir() {
            continue;
        }

        // Check result limits
        match mode {
            "files_with_matches" => {
                if files_matched.len() >= max {
                    break;
                }
            }
            "count" => {
                if count_results.len() >= max {
                    break;
                }
            }
            _ => {
                if matches.len() >= max {
                    break;
                }
            }
        }

        let rel_path = match entry.path().strip_prefix(path) {
            Ok(p) => p.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };

        // Apply glob filter
        if let Some(ref gm) = glob_matcher {
            let match_path = format!("{}/", rel_path);
            if !gm.is_match(&match_path) {
                continue;
            }
        }

        // Skip large files
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.len() > MAX_FILE_SIZE {
            continue;
        }

        let file = match std::fs::File::open(entry.path()) {
            Ok(f) => f,
            Err(_) => continue,
        };

        let reader = BufReader::new(file);
        let lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
        let mut file_match_count = 0usize;
        let mut file_has_match = false;

        for (line_idx, line) in lines.iter().enumerate() {
            if re.is_match(line) {
                file_has_match = true;
                file_match_count += 1;

                if mode == "content" {
                    if matches.len() >= max {
                        break;
                    }

                    // Add context lines
                    if ctx_lines > 0 {
                        let start = line_idx.saturating_sub(ctx_lines);
                        let end = (line_idx + ctx_lines + 1).min(lines.len());
                        for ctx_idx in start..end {
                            let sep = if ctx_idx == line_idx { ":" } else { "-" };
                            let entry_str = format!("{}{}{}:{}", rel_path, sep, ctx_idx + 1, lines[ctx_idx]);
                            total_output_bytes += entry_str.len();
                            if total_output_bytes > MAX_OUTPUT_BYTES {
                                break;
                            }
                            matches.push(entry_str);
                        }
                        // Add separator between context groups
                        matches.push("--".to_string());
                    } else {
                        let entry_str = format!("{}:{}:{}", rel_path, line_idx + 1, line);
                        total_output_bytes += entry_str.len();
                        if total_output_bytes > MAX_OUTPUT_BYTES {
                            break;
                        }
                        matches.push(entry_str);
                    }
                } else if mode == "files_with_matches" {
                    break; // One match is enough for this mode
                }
            }
        }

        if file_has_match {
            match mode {
                "files_with_matches" => {
                    files_matched.push(rel_path);
                }
                "count" => {
                    count_results.push(format!("{}:{}", rel_path, file_match_count));
                }
                _ => {} // Already added to matches
            }
        }

        if total_output_bytes > MAX_OUTPUT_BYTES {
            break;
        }
    }

    let mut result = match mode {
        "files_with_matches" => files_matched.join("\n"),
        "count" => count_results.join("\n"),
        _ => {
            // Remove trailing separator
            if matches.last().map(|s| s.as_str()) == Some("--") {
                matches.pop();
            }
            matches.join("\n")
        }
    };

    // Apply head_limit
    if let Some(limit) = head_limit {
        let lines: Vec<&str> = result.lines().take(limit).collect();
        result = lines.join("\n");
    }

    if result.len() > MAX_OUTPUT_BYTES {
        result.truncate(MAX_OUTPUT_BYTES);
        result.push_str("\n... (Truncated at 200KB)");
    }

    Ok(result)
}
