package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

const (
	defaultMaxResults   = 200
	defaultMaxReadLines = 400
	defaultMaxIters     = 50
	maxGrepFileSize     = 2 * 1024 * 1024 // 2MB
)

var (
	safeSessionRE = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$`)
)

// ToolResult represents the result of a tool execution
type ToolResult struct {
	OK      bool                   `json:"ok"`
	Tool    string                 `json:"tool,omitempty"`
	Error   string                 `json:"error,omitempty"`
	Results interface{}            `json:"results,omitempty"`
	Content string                 `json:"content,omitempty"`
	Count   int                    `json:"count,omitempty"`
	Path    string                 `json:"path,omitempty"`
	Extra   map[string]interface{} `json:",inline"`
}

// SessionData stores session state (response chaining via previous_response_id)
type SessionData struct {
	LastResponseID string `json:"last_response_id"`
}

func main() {
	if len(os.Args) < 3 {
		fmt.Fprintln(os.Stderr, `Usage: codex-review "<session-name>" "<review-prompt>"`)
		os.Exit(2)
	}

	sessionName := os.Args[1]
	reviewPrompt := strings.Join(os.Args[2:], " ")

	// Validate session name
	if !safeSessionRE.MatchString(sessionName) {
		fmt.Fprintln(os.Stderr, "Invalid session name: use A-Za-z0-9._- only, max 64 chars, must start with alphanumeric")
		os.Exit(2)
	}

	// Authentication: codeb credentials > OPENAI_API_KEY env
	apiKey := loadCodebToken()
	if apiKey == "" {
		apiKey = os.Getenv("OPENAI_API_KEY")
	}
	if apiKey == "" {
		fmt.Fprintln(os.Stderr, "No authentication found. Run 'codeb login' or set OPENAI_API_KEY")
		os.Exit(2)
	}

	model := getEnv("OPENAI_MODEL", "gpt-5.2-codex")
	reasoningEffort := getEnv("REASONING_EFFORT", "high") // Higher for code review
	maxIters := getEnvInt("MAX_ITERS", defaultMaxIters)

	// Detect repo root
	repoRoot, err := detectRepoRoot()
	if err != nil {
		fmt.Fprintf(os.Stderr, "Failed to detect repo root: %v\n", err)
		os.Exit(2)
	}

	// Session management
	sessionsDir := getEnv("STATE_DIR", filepath.Join(repoRoot, ".codex-sessions"))
	if err := os.MkdirAll(sessionsDir, 0755); err != nil {
		fmt.Fprintf(os.Stderr, "Failed to create sessions dir: %v\n", err)
		os.Exit(2)
	}

	sessionFile := filepath.Join(sessionsDir, sessionName+".json")

	// Load project memory (CLAUDE.md + rules) like Claude Code
	projectMemory := loadProjectMemory(repoRoot)
	systemPrompt := buildSystemPrompt(repoRoot, sessionName, projectMemory)

	// Load previous response ID for session continuity
	lastResponseID, _ := loadSession(sessionFile)

	// Execute review with tool loop (uses previous_response_id chaining)
	newResponseID, err := executeReview(apiKey, model, reasoningEffort, systemPrompt, lastResponseID, reviewPrompt, repoRoot, maxIters)
	if err != nil {
		fmt.Fprintf(os.Stderr, "%v\n", err)
		os.Exit(3)
	}

	// Save latest response ID for session resumption
	if newResponseID != "" {
		if err := saveSession(sessionFile, newResponseID); err != nil {
			fmt.Fprintf(os.Stderr, "Warning: failed to save session: %v\n", err)
		}
	}
}

// Helper functions
func getEnv(key, defaultVal string) string {
	if val := os.Getenv(key); val != "" {
		return val
	}
	return defaultVal
}

func getEnvInt(key string, defaultVal int) int {
	if val := os.Getenv(key); val != "" {
		if i, err := strconv.Atoi(val); err == nil {
			return i
		}
	}
	return defaultVal
}

func detectRepoRoot() (string, error) {
	if root := os.Getenv("REPO_ROOT"); root != "" {
		return filepath.Abs(root)
	}

	// Walk up to find .git directory
	cwd, err := os.Getwd()
	if err != nil {
		return "", err
	}

	dir := cwd
	for {
		if _, err := os.Stat(filepath.Join(dir, ".git")); err == nil {
			return dir, nil
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}

	// No git found, use cwd
	return cwd, nil
}

// loadCodebToken reads the aiproxy token from ~/.codeb/credentials.json
func loadCodebToken() string {
	homeDir, err := os.UserHomeDir()
	if err != nil {
		return ""
	}
	credPath := filepath.Join(homeDir, ".codeb", "credentials.json")
	data, err := os.ReadFile(credPath)
	if err != nil {
		return ""
	}
	var creds struct {
		Token string `json:"token"`
	}
	if err := json.Unmarshal(data, &creds); err != nil {
		return ""
	}
	return creds.Token
}

// loadProjectMemory loads CLAUDE.md and rules like Claude Code
// Priority: user memory -> user rules -> project memory -> project rules
func loadProjectMemory(repoRoot string) string {
	var sections []string
	homeDir, _ := os.UserHomeDir()

	// 1. User memory: ~/.claude/CLAUDE.md
	if homeDir != "" {
		userClaudePath := filepath.Join(homeDir, ".claude", "CLAUDE.md")
		if data, err := os.ReadFile(userClaudePath); err == nil {
			sections = append(sections, fmt.Sprintf("### %s (user memory)\n\n%s", userClaudePath, string(data)))
		}

		// 2. User rules: ~/.claude/rules/*.md
		userRulesDir := filepath.Join(homeDir, ".claude", "rules")
		if rules := loadRulesDir(userRulesDir, "user rules"); len(rules) > 0 {
			sections = append(sections, rules...)
		}
	}

	// 3. Project memory: .claude/CLAUDE.md or CLAUDE.md
	projectClaudePaths := []string{
		filepath.Join(repoRoot, ".claude", "CLAUDE.md"),
		filepath.Join(repoRoot, "CLAUDE.md"),
	}
	for _, p := range projectClaudePaths {
		if data, err := os.ReadFile(p); err == nil {
			relPath, _ := filepath.Rel(repoRoot, p)
			if relPath == "" {
				relPath = p
			}
			sections = append(sections, fmt.Sprintf("### %s (project memory)\n\n%s", relPath, string(data)))
			break // Only first found
		}
	}

	// 4. Project rules: .claude/rules/*.md
	projectRulesDir := filepath.Join(repoRoot, ".claude", "rules")
	if rules := loadRulesDir(projectRulesDir, "project rules"); len(rules) > 0 {
		sections = append(sections, rules...)
	}

	if len(sections) == 0 {
		return ""
	}

	return strings.Join(sections, "\n\n---\n\n")
}

// loadRulesDir loads all .md files from a rules directory
func loadRulesDir(rulesDir, ruleType string) []string {
	var rules []string

	entries, err := os.ReadDir(rulesDir)
	if err != nil {
		return rules
	}

	// Sort by filename (lower numbers = higher priority)
	var mdFiles []string
	for _, entry := range entries {
		if !entry.IsDir() && strings.HasSuffix(entry.Name(), ".md") {
			mdFiles = append(mdFiles, entry.Name())
		}
	}
	sort.Strings(mdFiles)

	for _, name := range mdFiles {
		path := filepath.Join(rulesDir, name)
		if data, err := os.ReadFile(path); err == nil {
			rules = append(rules, fmt.Sprintf("### %s (%s)\n\n%s", name, ruleType, string(data)))
		}
	}

	return rules
}
