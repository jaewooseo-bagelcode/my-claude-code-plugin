# Building codex-review

## Pre-built Binaries

Pre-built binaries are available in `bin/`:
- `codex-review-darwin-arm64` - macOS Apple Silicon (M1/M2/M3)

## Build from Source

### Prerequisites

- Go 1.21 or later
- Git (for detecting repo root)

### Build Steps

```bash
cd scripts
go mod download
go build -ldflags="-s -w" -o ../bin/codex-review-$(go env GOOS)-$(go env GOARCH)
```

### Platform-Specific Builds

**macOS (Apple Silicon)**:
```bash
GOOS=darwin GOARCH=arm64 go build -ldflags="-s -w" -o ../bin/codex-review-darwin-arm64
```

**macOS (Intel)**:
```bash
GOOS=darwin GOARCH=amd64 go build -ldflags="-s -w" -o ../bin/codex-review-darwin-amd64
```

**Linux (x86_64)**:
```bash
GOOS=linux GOARCH=amd64 go build -ldflags="-s -w" -o ../bin/codex-review-linux-amd64
```

**Linux (ARM64)**:
```bash
GOOS=linux GOARCH=arm64 go build -ldflags="-s -w" -o ../bin/codex-review-linux-arm64
```

**Windows (x86_64)**:
```bash
GOOS=windows GOARCH=amd64 go build -ldflags="-s -w" -o ../bin/codex-review-windows-amd64.exe
```

## Build Flags

- `-ldflags="-s -w"` - Strip debug info and symbol table (reduces binary size by ~30%)
- `-trimpath` - Remove absolute paths from binary (optional, for reproducible builds)

## Verify Build

```bash
./bin/codex-review-$(go env GOOS)-$(go env GOARCH) --help
# Should show: Usage: codex-review "<session-name>" "<review-prompt>"
```

## Binary Size

Typical sizes with `-ldflags="-s -w"`:
- macOS arm64: ~5.7MB
- Linux amd64: ~6.2MB
- Windows amd64: ~6.5MB

## Dependencies

Runtime dependencies: **None** (static binary)

Build dependencies:
- `golang.org/x/sys` - Platform-specific syscalls for security (openat on Unix)

See `go.mod` for exact versions.

## Security

The Go implementation provides stronger security than Python:
- openat syscalls on Unix (perfect TOCTOU/symlink protection)
- Strict validation on Windows (no openat, uses multiple checks)
- Zero dynamic dependencies (no supply chain risk at runtime)

See [SECURITY.md](SECURITY.md) for detailed security analysis.
