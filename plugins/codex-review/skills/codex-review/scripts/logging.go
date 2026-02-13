package main

import (
	"encoding/json"
	"os"
	"sync"
	"time"
)

// LogEntry represents a single JSONL log line
type LogEntry struct {
	Timestamp int64                  `json:"ts"`
	Event     string                 `json:"event"`
	Iteration int                    `json:"iteration"`
	Data      map[string]interface{} `json:"data,omitempty"`
}

// Logger writes structured JSONL logs (goroutine-safe, nil-safe)
type Logger struct {
	mu   sync.Mutex
	file *os.File
}

// NewLogger creates a logger writing to the given path. Returns nil on error (graceful).
func NewLogger(path string) *Logger {
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0644)
	if err != nil {
		return nil
	}
	return &Logger{file: f}
}

// Log writes one JSONL entry. No-op if logger is nil.
func (l *Logger) Log(event string, iteration int, data map[string]interface{}) {
	if l == nil {
		return
	}
	entry := LogEntry{
		Timestamp: time.Now().UnixMilli(),
		Event:     event,
		Iteration: iteration,
		Data:      data,
	}
	line, err := json.Marshal(entry)
	if err != nil {
		return
	}
	line = append(line, '\n')

	l.mu.Lock()
	defer l.mu.Unlock()
	l.file.Write(line)
}

// Close flushes and closes the log file. No-op if logger is nil.
func (l *Logger) Close() {
	if l == nil {
		return
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	l.file.Close()
}

// summarizeArgs truncates argument strings to maxLen characters
func summarizeArgs(s string, maxLen int) string {
	if len(s) <= maxLen {
		return s
	}
	return s[:maxLen] + "..."
}
