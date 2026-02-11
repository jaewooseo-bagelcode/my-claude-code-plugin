#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BIN_DIR="$(dirname "$SCRIPT_DIR")/bin"

echo "Building braintrust..."

cd "$SCRIPT_DIR"
cargo build --release

mkdir -p "$BIN_DIR"
cp target/release/braintrust "$BIN_DIR/braintrust-darwin-arm64"
chmod +x "$BIN_DIR/braintrust-darwin-arm64"

echo "Binary: $BIN_DIR/braintrust-darwin-arm64"
echo "Size: $(du -h "$BIN_DIR/braintrust-darwin-arm64" | cut -f1)"
echo "Done."
