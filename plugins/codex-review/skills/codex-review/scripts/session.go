package main

import (
	"encoding/json"
	"os"
	"path/filepath"
)

// loadSession loads conversation ID from session file
func loadSession(sessionFile string) (string, error) {
	data, err := os.ReadFile(sessionFile)
	if err != nil {
		if os.IsNotExist(err) {
			return "", nil
		}
		return "", err
	}

	var session SessionData
	if err := json.Unmarshal(data, &session); err != nil {
		return "", err
	}

	return session.LastResponseID, nil
}

// saveSession atomically saves conversation ID to session file
func saveSession(sessionFile, lastResponseID string) error {
	session := SessionData{LastResponseID: lastResponseID}
	data, err := json.MarshalIndent(session, "", "  ")
	if err != nil {
		return err
	}

	// Atomic write: temp file + rename
	dir := filepath.Dir(sessionFile)
	tmpFile, err := os.CreateTemp(dir, "session-*.tmp")
	if err != nil {
		return err
	}
	tmpPath := tmpFile.Name()

	defer func() {
		tmpFile.Close()
		os.Remove(tmpPath) // Clean up on error
	}()

	if _, err := tmpFile.Write(data); err != nil {
		return err
	}

	if err := tmpFile.Close(); err != nil {
		return err
	}

	// Atomic rename
	return os.Rename(tmpPath, sessionFile)
}
