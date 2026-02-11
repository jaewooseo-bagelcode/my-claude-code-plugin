package main

import (
	"bufio"
	"context"
	"fmt"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"time"
)

var safeBranchRE = regexp.MustCompile(`^[A-Za-z0-9._/\-]+$`)

// globMatcher is a precompiled glob pattern compiled to a regex for O(n) matching.
type globMatcher struct {
	re *regexp.Regexp
}

// compileGlob converts a glob pattern with ** support into a regex, compiled once per call.
func compileGlob(pattern string) *globMatcher {
	pattern = filepath.ToSlash(pattern)
	parts := strings.Split(pattern, "/")
	var reParts []string
	for _, part := range parts {
		if part == "**" {
			reParts = append(reParts, "(?:.+/)?") // zero or more dirs
		} else {
			reParts = append(reParts, globSegmentToRegex(part))
		}
	}
	// Join with / and anchor
	reStr := "^" + strings.Join(reParts, "") + "$"
	// Clean up double slashes from ** joining
	reStr = strings.ReplaceAll(reStr, "(?:.+/)?/", "(?:.+/)?")
	re, err := regexp.Compile(reStr)
	if err != nil {
		// Fallback: match nothing
		re = regexp.MustCompile(`\z.`)
	}
	return &globMatcher{re: re}
}

// globSegmentToRegex converts a single glob segment (e.g. *.ts) to regex + trailing /
func globSegmentToRegex(seg string) string {
	var b strings.Builder
	for i := 0; i < len(seg); i++ {
		ch := seg[i]
		switch ch {
		case '*':
			b.WriteString("[^/]*")
		case '?':
			b.WriteString("[^/]")
		case '[':
			// Pass character class through
			j := strings.Index(seg[i:], "]")
			if j > 0 {
				b.WriteString(seg[i : i+j+1])
				i += j
			} else {
				b.WriteString(regexp.QuoteMeta(string(ch)))
			}
		default:
			b.WriteString(regexp.QuoteMeta(string(ch)))
		}
	}
	return b.String() + "/"
}

func (g *globMatcher) match(path string) bool {
	// Append trailing / so segment regexes can match uniformly
	return g.re.MatchString(filepath.ToSlash(path) + "/")
}

// skipDir returns true if directory should be skipped during walk
func skipDir(name string) bool {
	return name == ".git" || name == "node_modules" || name == ".venv" ||
		name == "__pycache__" || name == ".codex-sessions"
}

// toolGlob finds files matching a pattern with ** recursive support
func toolGlob(repoRoot, pattern string, maxResults int) ToolResult {
	if pattern == "" {
		return ToolResult{OK: false, Error: "Glob: pattern required"}
	}

	if maxResults <= 0 || maxResults > defaultMaxResults {
		maxResults = defaultMaxResults
	}

	// Precompile glob pattern to regex once
	matcher := compileGlob(pattern)
	results := []string{}

	filepath.WalkDir(repoRoot, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return nil
		}

		if d.IsDir() {
			if skipDir(d.Name()) {
				return filepath.SkipDir
			}
			return nil
		}

		if len(results) >= maxResults {
			return fs.SkipAll
		}

		// Get relative path from repo root
		relPath, err := filepath.Rel(repoRoot, path)
		if err != nil {
			return nil
		}
		relPath = filepath.ToSlash(relPath)

		if !matcher.match(relPath) {
			return nil
		}

		results = append(results, relPath)
		return nil
	})

	return ToolResult{
		OK:      true,
		Tool:    "Glob",
		Results: results,
		Count:   len(results),
		Extra:   map[string]interface{}{"repo_root": repoRoot, "pattern": pattern},
	}
}

// toolRead reads a file with line range
func toolRead(repoRoot, path string, startLine, endLine, maxLines int) ToolResult {
	if path == "" {
		return ToolResult{OK: false, Error: "Read: path required"}
	}

	absPath := filepath.Join(repoRoot, path)
	file, err := os.Open(absPath)
	if err != nil {
		return ToolResult{OK: false, Error: fmt.Sprintf("Read: %v", err)}
	}
	defer file.Close()

	// Verify it's a regular file
	info, err := file.Stat()
	if err != nil {
		return ToolResult{OK: false, Error: fmt.Sprintf("Read: %v", err)}
	}
	if !info.Mode().IsRegular() {
		return ToolResult{OK: false, Error: "Read: not a regular file"}
	}

	// Read lines
	if maxLines <= 0 || maxLines > defaultMaxReadLines {
		maxLines = defaultMaxReadLines
	}
	if startLine < 1 {
		startLine = 1
	}
	if endLine <= 0 {
		endLine = startLine + maxLines - 1
	}
	if endLine < startLine {
		endLine = startLine
	}
	if endLine-startLine+1 > maxLines {
		endLine = startLine + maxLines - 1
	}

	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, 64*1024), 1024*1024) // 1MB line limit
	lines := []string{}
	lineNum := 0

	for scanner.Scan() {
		lineNum++
		if lineNum < startLine {
			continue
		}
		if lineNum > endLine {
			break
		}
		lines = append(lines, fmt.Sprintf("%06d\t%s", lineNum, scanner.Text()))
	}

	if err := scanner.Err(); err != nil {
		return ToolResult{OK: false, Error: fmt.Sprintf("Read: %v", err)}
	}

	return ToolResult{
		OK:      true,
		Tool:    "Read",
		Path:    path,
		Content: strings.Join(lines, "\n"),
		Extra: map[string]interface{}{
			"start":     startLine,
			"end":       endLine,
			"repo_root": repoRoot,
		},
	}
}

// toolGrep searches for text/regex in files
func toolGrep(repoRoot, query, globFilter string, maxResults int) ToolResult {
	if query == "" {
		return ToolResult{OK: false, Error: "Grep: query required"}
	}

	if maxResults <= 0 || maxResults > defaultMaxResults {
		maxResults = defaultMaxResults
	}

	// Compile regex; fallback to literal match on invalid regex
	re, err := regexp.Compile(query)
	if err != nil {
		re = regexp.MustCompile(regexp.QuoteMeta(query))
	}

	// Precompile glob filter to regex once
	var globMatcher *globMatcher
	if globFilter != "" {
		globMatcher = compileGlob(globFilter)
	}

	// Walk files from repoRoot directly
	matches := []string{}
	filepath.WalkDir(repoRoot, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return nil
		}

		if d.IsDir() {
			if skipDir(d.Name()) {
				return filepath.SkipDir
			}
			return nil
		}

		// Get relative path
		relPath, relErr := filepath.Rel(repoRoot, path)
		if relErr != nil {
			return nil
		}
		relPath = filepath.ToSlash(relPath)

		// Apply precompiled glob filter with ** support
		if globMatcher != nil {
			if !globMatcher.match(relPath) {
				return nil
			}
		}

		// Skip large files
		info, statErr := d.Info()
		if statErr != nil {
			return nil
		}
		if info.Size() > maxGrepFileSize {
			return nil
		}

		if len(matches) >= maxResults {
			return fs.SkipAll
		}

		file, openErr := os.Open(path)
		if openErr != nil {
			return nil
		}
		defer file.Close()

		scanner := bufio.NewScanner(file)
		scanner.Buffer(make([]byte, 64*1024), 1024*1024) // 1MB line limit
		lineNum := 0
		for scanner.Scan() {
			lineNum++
			if len(matches) >= maxResults {
				break
			}
			line := scanner.Text()
			if re.MatchString(line) {
				matches = append(matches, fmt.Sprintf("%s:%d:%s", relPath, lineNum, line))
			}
		}
		_ = scanner.Err()

		return nil
	})

	return ToolResult{
		OK:      true,
		Tool:    "Grep",
		Results: matches,
		Count:   len(matches),
		Extra: map[string]interface{}{
			"repo_root": repoRoot,
			"query":     query,
			"glob":      globFilter,
		},
	}
}

const maxDiffLines = 10000

// toolGitDiff gets git diff of changes since a base branch (streaming, memory-efficient)
func toolGitDiff(repoRoot, base, path string) ToolResult {
	if base == "" {
		base = "main"
	}

	if !safeBranchRE.MatchString(base) {
		return ToolResult{OK: false, Error: "GitDiff: invalid base branch name"}
	}

	args := []string{"diff", base + "...HEAD"}

	if path != "" {
		args = append(args, "--", path)
	}

	content, truncated, err := runGitDiffStreaming(repoRoot, args)
	if err != nil {
		// Fallback: try simple diff against base
		args2 := []string{"diff", base}
		if path != "" {
			args2 = append(args2, "--", path)
		}
		content, truncated, err = runGitDiffStreaming(repoRoot, args2)
		if err != nil {
			return ToolResult{OK: false, Error: fmt.Sprintf("GitDiff: %v", err)}
		}
	}

	return ToolResult{
		OK:      true,
		Tool:    "GitDiff",
		Content: content,
		Extra: map[string]interface{}{
			"repo_root": repoRoot,
			"base":      base,
			"path":      path,
			"truncated": truncated,
		},
	}
}

// runGitDiffStreaming runs git diff and streams output, stopping at maxDiffLines.
func runGitDiffStreaming(repoRoot string, args []string) (string, bool, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()

	cmd := exec.CommandContext(ctx, "git", args...)
	cmd.Dir = repoRoot

	pipe, err := cmd.StdoutPipe()
	if err != nil {
		return "", false, err
	}
	cmd.Stderr = nil // discard stderr

	if err := cmd.Start(); err != nil {
		return "", false, err
	}

	scanner := bufio.NewScanner(pipe)
	scanner.Buffer(make([]byte, 64*1024), 1024*1024)

	var b strings.Builder
	lineCount := 0
	truncated := false

	for scanner.Scan() {
		line := scanner.Text()
		lineCount++
		if lineCount > maxDiffLines {
			truncated = true
			break
		}
		b.WriteString(line)
		b.WriteByte('\n')
	}

	// Kill process early if truncated (don't wait for full output)
	if truncated {
		cmd.Process.Kill()
	}
	waitErr := cmd.Wait()

	content := strings.TrimRight(b.String(), "\n")
	if truncated {
		content += fmt.Sprintf("\n\n... truncated (showing %d of total lines)", maxDiffLines)
	}

	// If command failed and produced no output, propagate the error
	// so callers can try fallback strategies
	if !truncated && content == "" && waitErr != nil {
		return "", false, waitErr
	}

	return content, truncated, nil
}

// executeTool dispatches tool execution (READ-ONLY tools + GitDiff)
func executeTool(repoRoot, toolName string, args map[string]interface{}) ToolResult {
	switch toolName {
	case "Glob":
		pattern, _ := args["pattern"].(string)
		maxResults, _ := args["max_results"].(float64)
		return toolGlob(repoRoot, pattern, int(maxResults))

	case "Read":
		path, _ := args["path"].(string)
		startLine, _ := args["start_line"].(float64)
		endLine, _ := args["end_line"].(float64)
		maxLines, _ := args["max_lines"].(float64)
		return toolRead(repoRoot, path, int(startLine), int(endLine), int(maxLines))

	case "Grep":
		query, _ := args["query"].(string)
		glob, _ := args["glob"].(string)
		maxResults, _ := args["max_results"].(float64)
		return toolGrep(repoRoot, query, glob, int(maxResults))

	case "GitDiff":
		base, _ := args["base"].(string)
		path, _ := args["path"].(string)
		return toolGitDiff(repoRoot, base, path)

	default:
		return ToolResult{OK: false, Error: fmt.Sprintf("Unknown tool: %s (only Glob, Grep, Read, GitDiff allowed)", toolName)}
	}
}
