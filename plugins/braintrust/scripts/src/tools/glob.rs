use glob::glob as glob_match;
use serde::Serialize;

#[derive(Serialize)]
struct GlobResult {
    count: usize,
    matches: Vec<String>,
    search_path: String,
}

pub fn glob_files(pattern: &str, path: &str) -> Result<String, String> {
    let full_pattern = format!("{}/{}", path.trim_end_matches('/'), pattern);

    let mut matches: Vec<String> = glob_match(&full_pattern)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    matches.sort();

    let result = GlobResult {
        count: matches.len(),
        matches,
        search_path: path.to_string(),
    };

    serde_json::to_string(&result).map_err(|e| e.to_string())
}
