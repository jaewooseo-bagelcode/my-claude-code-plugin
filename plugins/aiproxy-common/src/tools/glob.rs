use regex::Regex;
use serde::Serialize;
use walkdir::WalkDir;

const DEFAULT_MAX_RESULTS: usize = 200;

/// Directories to skip during walk (Go skipDir + Rust deny list merged)
fn skip_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | "node_modules" | ".venv" | "__pycache__" | ".codex-sessions" | ".svn" | ".hg"
    )
}

/// Compile a glob pattern with ** support into a regex (ported from Go compileGlob).
fn compile_glob(pattern: &str) -> Option<Regex> {
    let pattern = pattern.replace('\\', "/");
    let parts: Vec<&str> = pattern.split('/').collect();
    let mut re_parts = Vec::new();

    for part in &parts {
        if *part == "**" {
            re_parts.push("(?:.+/)?".to_string()); // zero or more dirs
        } else {
            re_parts.push(format!("{}/", glob_segment_to_regex(part)));
        }
    }

    let mut re_str = format!("^{}", re_parts.join(""));
    // Clean up double slashes from ** joining
    re_str = re_str.replace("(?:.+/)?/", "(?:.+/)?");
    re_str.push('$');

    Regex::new(&re_str).ok()
}

/// Convert a single glob segment (e.g. *.ts) to regex
fn glob_segment_to_regex(seg: &str) -> String {
    let mut b = String::new();
    let chars: Vec<char> = seg.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '*' => b.push_str("[^/]*"),
            '?' => b.push_str("[^/]"),
            '[' => {
                // Pass character class through
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

#[derive(Serialize)]
struct GlobResult {
    ok: bool,
    tool: &'static str,
    count: usize,
    results: Vec<String>,
    repo_root: String,
    pattern: String,
}

pub fn execute(pattern: &str, path: &str, max_results: Option<usize>) -> Result<String, String> {
    if pattern.is_empty() {
        return Err("Glob: pattern required".to_string());
    }

    let max = max_results.unwrap_or(DEFAULT_MAX_RESULTS).min(DEFAULT_MAX_RESULTS);

    let matcher = compile_glob(pattern)
        .ok_or_else(|| format!("Glob: invalid pattern '{}'", pattern))?;

    let mut results = Vec::new();

    let walker = WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            // Skip denied directories
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

        if results.len() >= max {
            break;
        }

        let rel_path = match entry.path().strip_prefix(path) {
            Ok(p) => p.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };

        // Append trailing / for matching (as Go does)
        let match_path = format!("{}/", rel_path);
        if matcher.is_match(&match_path) {
            results.push(rel_path);
        }
    }

    let result = GlobResult {
        ok: true,
        tool: "Glob",
        count: results.len(),
        results,
        repo_root: path.to_string(),
        pattern: pattern.to_string(),
    };

    serde_json::to_string(&result).map_err(|e| e.to_string())
}
