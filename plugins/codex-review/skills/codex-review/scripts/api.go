package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
)

const (
	defaultAPIBase = "https://aiproxy-api.backoffice.bagelgames.com/openai/v1"
)

// getAPIBase returns the API base URL (configurable via env)
func getAPIBase() string {
	if base := os.Getenv("AIPROXY_API_BASE"); base != "" {
		return strings.TrimRight(base, "/")
	}
	return defaultAPIBase
}

// buildSystemPrompt loads system-prompt-en.md and substitutes variables
func buildSystemPrompt(repoRoot, sessionName, projectMemory string) string {
	scriptDir, _ := filepath.Abs(filepath.Dir(os.Args[0]))
	promptPath := filepath.Join(scriptDir, "system-prompt-en.md")

	template, err := os.ReadFile(promptPath)
	if err != nil {
		// Fallback inline prompt
		return fmt.Sprintf(`# Code Review Expert - GPT-5.2-Codex

You are a professional code reviewer with extensive experience.

Repository Root: %s
Session: %s

## Project Guidelines

%s

---

**CRITICAL: You provide READ-ONLY analysis.** Identify issues and provide suggestions, but do NOT modify code.

Available Tools: Glob (supports **), Grep (supports regex), Read, GitDiff

Analyze code across 5 dimensions:
- 🐛 Bugs (Critical)
- 🔒 Security (High)
- ⚡ Performance (Medium)
- 📝 Code Quality (Low)
- 🔧 Refactoring

Provide detailed markdown reports with actionable suggestions.
`, repoRoot, sessionName, projectMemory)
	}

	prompt := string(template)
	prompt = strings.ReplaceAll(prompt, "{repo_root}", repoRoot)
	prompt = strings.ReplaceAll(prompt, "{session_name}", sessionName)
	prompt = strings.ReplaceAll(prompt, "{project_memory}", projectMemory)

	return prompt
}

// getToolsSchema returns OpenAI function tool definitions (READ-ONLY)
func getToolsSchema() []map[string]interface{} {
	return []map[string]interface{}{
		{
			"type": "function",
			"name": "Glob",
			"description": "Find repository files matching a glob pattern relative to repo root. Supports ** for recursive directory matching.",
			"parameters": map[string]interface{}{
				"type": "object",
				"properties": map[string]interface{}{
					"pattern": map[string]interface{}{
						"type":        "string",
						"description": "Glob pattern like src/**/*.ts or **/*.go (relative to repo root). ** matches zero or more directories.",
					},
					"max_results": map[string]interface{}{
						"type":        "integer",
						"description": "Max results (<=200). Default 200.",
					},
				},
				"required": []string{"pattern"},
			},
		},
		{
			"type": "function",
			"name": "Grep",
			"description": "Search for text or regex patterns in repository files; optionally restrict to a glob.",
			"parameters": map[string]interface{}{
				"type": "object",
				"properties": map[string]interface{}{
					"query": map[string]interface{}{
						"type":        "string",
						"description": "Search query (supports regex, e.g. 'handleRate.*Limit' or 'async\\s+function'). Falls back to literal match if regex is invalid.",
					},
					"glob": map[string]interface{}{
						"type":        "string",
						"description": "Optional file glob scope like src/**/*.ts (supports ** recursive matching)",
					},
					"max_results": map[string]interface{}{
						"type":        "integer",
						"description": "Max matches (<=200). Default 200.",
					},
				},
				"required": []string{"query"},
			},
		},
		{
			"type": "function",
			"name": "Read",
			"description": "Read a file snippet by line range (relative path).",
			"parameters": map[string]interface{}{
				"type": "object",
				"properties": map[string]interface{}{
					"path": map[string]interface{}{
						"type":        "string",
						"description": "Relative file path from repo root.",
					},
					"start_line": map[string]interface{}{
						"type":        "integer",
						"description": "1-based start line. Default 1.",
					},
					"end_line": map[string]interface{}{
						"type":        "integer",
						"description": "1-based end line (inclusive).",
					},
					"max_lines": map[string]interface{}{
						"type":        "integer",
						"description": "Max lines to return (<=400). Default 400.",
					},
				},
				"required": []string{"path"},
			},
		},
		{
			"type": "function",
			"name": "GitDiff",
			"description": "Get git diff of changes since a base branch. Useful for reviewing PR changes.",
			"parameters": map[string]interface{}{
				"type": "object",
				"properties": map[string]interface{}{
					"base": map[string]interface{}{
						"type":        "string",
						"description": "Base branch to diff against (default: 'main'). Examples: 'main', 'develop', 'origin/main'.",
					},
					"path": map[string]interface{}{
						"type":        "string",
						"description": "Optional file path to restrict diff to a specific file.",
					},
				},
				"required": []string{},
			},
		},
	}
}

// executeReview runs the tool execution loop for code review.
// Uses previous_response_id chaining instead of conversations API.
// Returns the last response ID for session persistence.
func executeReview(apiKey, model, reasoningEffort, systemPrompt, previousResponseID, reviewPrompt, repoRoot string, maxIters int) (string, error) {
	ctx := context.Background()
	tools := getToolsSchema()

	var lastResponseID string

	// Initial input: user's review prompt
	inputItems := []map[string]interface{}{
		{
			"role":    "user",
			"content": reviewPrompt,
		},
	}

	for iteration := 0; iteration < maxIters; iteration++ {
		// Build payload
		payload := map[string]interface{}{
			"model":               model,
			"tools":               tools,
			"tool_choice":         "auto",
			"parallel_tool_calls": true,
			"input":               inputItems,
		}

		// System prompt via instructions field (always sent for context)
		if systemPrompt != "" {
			payload["instructions"] = systemPrompt
		}

		// Chain to previous response for session continuity
		if lastResponseID != "" {
			payload["previous_response_id"] = lastResponseID
		} else if previousResponseID != "" {
			payload["previous_response_id"] = previousResponseID
		}

		if reasoningEffort != "" {
			payload["reasoning"] = map[string]interface{}{
				"effort": reasoningEffort,
			}
		}

		// Call Responses API
		respData, err := callResponsesAPI(ctx, apiKey, payload)
		if err != nil {
			return lastResponseID, fmt.Errorf("API error: %w", err)
		}

		// Track response ID for chaining
		if id, ok := respData["id"].(string); ok {
			lastResponseID = id
		}

		// Extract tool calls and text
		toolCalls, outputText := extractCallsAndText(respData)

		// Print output text
		if outputText != "" {
			fmt.Print(outputText)
		}

		if len(toolCalls) == 0 {
			// No tool calls => review complete
			return lastResponseID, nil
		}

		// Execute tool calls in parallel
		type indexedOutput struct {
			index  int
			output map[string]interface{}
		}

		outputs := make([]map[string]interface{}, len(toolCalls))
		var wg sync.WaitGroup
		ch := make(chan indexedOutput, len(toolCalls))

		for i, call := range toolCalls {
			callID, ok := call["call_id"].(string)
			if !ok {
				outputs[i] = map[string]interface{}{
					"type":    "function_call_output",
					"call_id": "",
					"output":  `{"ok": false, "error": "missing call_id"}`,
				}
				continue
			}
			name, ok := call["name"].(string)
			if !ok {
				outputs[i] = map[string]interface{}{
					"type":    "function_call_output",
					"call_id": callID,
					"output":  `{"ok": false, "error": "missing tool name"}`,
				}
				continue
			}
			argsStr, ok := call["arguments"].(string)
			if !ok {
				argsStr = "{}"
			}

			wg.Add(1)
			go func(idx int, cID, tName, aStr string) {
				defer wg.Done()

				var args map[string]interface{}
				if err := json.Unmarshal([]byte(aStr), &args); err != nil {
					ch <- indexedOutput{idx, map[string]interface{}{
						"type":    "function_call_output",
						"call_id": cID,
						"output":  fmt.Sprintf(`{"ok": false, "error": "Invalid arguments: %v"}`, err),
					}}
					return
				}

				result := executeTool(repoRoot, tName, args)
				resultJSON, _ := json.Marshal(result)

				ch <- indexedOutput{idx, map[string]interface{}{
					"type":    "function_call_output",
					"call_id": cID,
					"output":  string(resultJSON),
				}}
			}(i, callID, name, argsStr)
		}

		// Collect results
		go func() {
			wg.Wait()
			close(ch)
		}()
		for out := range ch {
			outputs[out.index] = out.output
		}

		// Filter nil entries (from skipped calls)
		filteredOutputs := make([]map[string]interface{}, 0, len(outputs))
		for _, o := range outputs {
			if o != nil {
				filteredOutputs = append(filteredOutputs, o)
			}
		}

		inputItems = filteredOutputs
	}

	return lastResponseID, fmt.Errorf("reached MAX_ITERS=%d without completion", maxIters)
}

// callResponsesAPI makes HTTP request to Responses API
func callResponsesAPI(ctx context.Context, apiKey string, payload map[string]interface{}) (map[string]interface{}, error) {
	data, err := json.Marshal(payload)
	if err != nil {
		return nil, err
	}

	req, err := http.NewRequestWithContext(ctx, "POST", getAPIBase()+"/responses", bytes.NewReader(data))
	if err != nil {
		return nil, err
	}

	req.Header.Set("Authorization", "Bearer "+apiKey)
	req.Header.Set("Content-Type", "application/json")

	client := &http.Client{Timeout: 0} // no timeout
	resp, err := client.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, err
	}

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("HTTP %d: %s", resp.StatusCode, string(body[:min(2000, len(body))]))
	}

	var result map[string]interface{}
	if err := json.Unmarshal(body, &result); err != nil {
		return nil, err
	}

	return result, nil
}

// extractCallsAndText parses response output
func extractCallsAndText(resp map[string]interface{}) ([]map[string]interface{}, string) {
	calls := []map[string]interface{}{}
	texts := []string{}

	output, ok := resp["output"].([]interface{})
	if !ok {
		return calls, ""
	}

	for _, item := range output {
		itemMap, ok := item.(map[string]interface{})
		if !ok {
			continue
		}

		itemType, _ := itemMap["type"].(string)

		if itemType == "function_call" {
			calls = append(calls, map[string]interface{}{
				"name":      itemMap["name"],
				"call_id":   itemMap["call_id"],
				"arguments": itemMap["arguments"],
			})
		} else if itemType == "message" {
			content, ok := itemMap["content"].([]interface{})
			if !ok {
				continue
			}
			for _, c := range content {
				cMap, ok := c.(map[string]interface{})
				if !ok {
					continue
				}
				if cMap["type"] == "output_text" {
					if text, ok := cMap["text"].(string); ok {
						texts = append(texts, text)
					}
				}
			}
		}
	}

	return calls, strings.Join(texts, "")
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}
