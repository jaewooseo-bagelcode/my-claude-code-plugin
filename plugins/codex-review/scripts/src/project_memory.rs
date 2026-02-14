use std::path::PathBuf;

/// Load project memory (CLAUDE.md + rules) like Claude Code.
/// Priority:
/// 1. ~/.claude/CLAUDE.md (user memory)
/// 2. ~/.claude/rules/*.md (user rules, sorted by filename)
/// 3. {repo}/.claude/CLAUDE.md or {repo}/CLAUDE.md (project memory)
/// 4. {repo}/.claude/rules/*.md (project rules, sorted by filename)
pub fn load_project_memory(repo_root: &str) -> String {
    let mut sections = Vec::new();
    let home_dir = dirs::home_dir();

    // 1. User memory: ~/.claude/CLAUDE.md
    if let Some(ref home) = home_dir {
        let user_claude_path = home.join(".claude/CLAUDE.md");
        if let Ok(data) = std::fs::read_to_string(&user_claude_path) {
            sections.push(format!(
                "### {} (user memory)\n\n{}",
                user_claude_path.display(),
                data
            ));
        }

        // 2. User rules: ~/.claude/rules/*.md
        let user_rules_dir = home.join(".claude/rules");
        sections.extend(load_rules_dir(&user_rules_dir, "user rules"));
    }

    // 3. Project memory: .claude/CLAUDE.md or CLAUDE.md
    let project_root = PathBuf::from(repo_root);
    let project_claude_paths = [
        project_root.join(".claude/CLAUDE.md"),
        project_root.join("CLAUDE.md"),
    ];
    for path in &project_claude_paths {
        if let Ok(data) = std::fs::read_to_string(path) {
            let rel_path = path
                .strip_prefix(repo_root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| path.to_string_lossy().to_string());
            sections.push(format!("### {} (project memory)\n\n{}", rel_path, data));
            break; // Only first found
        }
    }

    // 4. Project rules: .claude/rules/*.md
    let project_rules_dir = project_root.join(".claude/rules");
    sections.extend(load_rules_dir(&project_rules_dir, "project rules"));

    if sections.is_empty() {
        String::new()
    } else {
        sections.join("\n\n---\n\n")
    }
}

/// Load all .md files from a rules directory, sorted by filename.
fn load_rules_dir(rules_dir: &PathBuf, rule_type: &str) -> Vec<String> {
    let mut rules = Vec::new();

    let entries = match std::fs::read_dir(rules_dir) {
        Ok(e) => e,
        Err(_) => return rules,
    };

    let mut md_files: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(true) && name.ends_with(".md") {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    md_files.sort();

    for name in &md_files {
        let path = rules_dir.join(name);
        if let Ok(data) = std::fs::read_to_string(&path) {
            rules.push(format!("### {} ({})\n\n{}", name, rule_type, data));
        }
    }

    rules
}

/// Build the system prompt by loading the embedded template and substituting variables.
pub fn build_system_prompt(repo_root: &str, session_name: &str, project_memory: &str) -> String {
    const SYSTEM_PROMPT: &str = include_str!("../../bin/system-prompt-en.md");

    SYSTEM_PROMPT
        .replace("{repo_root}", repo_root)
        .replace("{session_name}", session_name)
        .replace("{project_memory}", project_memory)
}
