use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIProxyConfig {
    pub base_url: String,
    pub token: String,
    pub no_aiproxy: bool,
    #[serde(skip)]
    pub anthropic_api_key: Option<String>,
    #[serde(skip)]
    pub openai_api_key: Option<String>,
    #[serde(skip)]
    pub gemini_api_key: Option<String>,
}

impl AIProxyConfig {
    pub fn anthropic_url(&self, path: &str) -> String {
        if self.no_aiproxy {
            format!("https://api.anthropic.com{}", path)
        } else {
            format!("{}/anthropic{}", self.base_url.trim_end_matches('/'), path)
        }
    }

    pub fn anthropic_auth(&self) -> (&str, String) {
        if self.no_aiproxy {
            ("x-api-key", self.anthropic_api_key.clone().unwrap_or_default())
        } else {
            ("Authorization", format!("Bearer {}", self.token))
        }
    }

    pub fn openai_url(&self, path: &str) -> String {
        if self.no_aiproxy {
            format!("https://api.openai.com{}", path)
        } else {
            format!("{}/openai{}", self.base_url.trim_end_matches('/'), path)
        }
    }

    pub fn openai_token(&self) -> &str {
        if self.no_aiproxy {
            self.openai_api_key.as_deref().unwrap_or("")
        } else {
            &self.token
        }
    }

    pub fn gemini_url(&self, path: &str) -> String {
        if self.no_aiproxy {
            format!("https://generativelanguage.googleapis.com{}", path)
        } else {
            format!("{}/google{}", self.base_url.trim_end_matches('/'), path)
        }
    }

    pub fn gemini_auth(&self) -> (&str, String) {
        if self.no_aiproxy {
            ("x-goog-api-key", self.gemini_api_key.clone().unwrap_or_default())
        } else {
            ("Authorization", format!("Bearer {}", self.token))
        }
    }
}

fn load_codeb_token() -> Result<String, String> {
    let home = dirs::home_dir().ok_or("No home directory")?;
    let creds_path = home.join(".codeb/credentials.json");

    let content = std::fs::read_to_string(&creds_path)
        .map_err(|e| format!("Failed to read {}: {}", creds_path.display(), e))?;

    let creds: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Invalid JSON in credentials: {}", e))?;

    creds
        .get("token")
        .and_then(|t| t.as_str())
        .map(|t| t.to_string())
        .ok_or_else(|| "token field not found in credentials.json".to_string())
}

pub fn load_config() -> Result<AIProxyConfig, String> {
    let no_aiproxy = std::env::var("NO_AIPROXY")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    if no_aiproxy {
        let anthropic_key = std::env::var("ANTHROPIC_API_KEY").ok();
        let openai_key = std::env::var("OPENAI_API_KEY").ok();
        let gemini_key = std::env::var("GEMINI_API_KEY").ok();

        if anthropic_key.is_none() && openai_key.is_none() && gemini_key.is_none() {
            return Err("NO_AIPROXY=1 requires at least one of ANTHROPIC_API_KEY, OPENAI_API_KEY, GEMINI_API_KEY".to_string());
        }

        Ok(AIProxyConfig {
            base_url: String::new(),
            token: String::new(),
            no_aiproxy: true,
            anthropic_api_key: anthropic_key,
            openai_api_key: openai_key,
            gemini_api_key: gemini_key,
        })
    } else {
        let base_url = std::env::var("AI_PROXY_BASE_URL")
            .unwrap_or_else(|_| "https://aiproxy-api.backoffice.bagelgames.com".to_string());

        let token = load_codeb_token()
            .or_else(|_| std::env::var("AI_PROXY_PERSONAL_TOKEN"))
            .map_err(|_| "codeb login required. Run 'codeb login' first.".to_string())?;

        Ok(AIProxyConfig {
            base_url,
            token,
            no_aiproxy: false,
            anthropic_api_key: None,
            openai_api_key: None,
            gemini_api_key: None,
        })
    }
}

/// Load project memory files (CLAUDE.md, .claude/rules/*.md) as context
pub fn load_project_memory(project_path: &str) -> Option<String> {
    let project = PathBuf::from(project_path);
    let mut memory_parts = Vec::new();

    // CLAUDE.md
    let claude_md = project.join("CLAUDE.md");
    if let Ok(content) = std::fs::read_to_string(&claude_md) {
        memory_parts.push(format!("## CLAUDE.md\n{}", content));
    }

    // .claude/rules/*.md
    let rules_dir = project.join(".claude/rules");
    if rules_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&rules_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "md").unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        memory_parts.push(format!("## {}\n{}", name, content));
                    }
                }
            }
        }
    }

    if memory_parts.is_empty() {
        None
    } else {
        Some(memory_parts.join("\n\n"))
    }
}
