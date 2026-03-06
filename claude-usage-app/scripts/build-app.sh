#!/bin/bash
set -euo pipefail

IDENTITY="Developer ID Application: Sugarscone (5FK7UUGMX3)"
TEAM_ID="5FK7UUGMX3"
BUNDLE_ID="com.sugarscone.claude-usage"
APP="ClaudeUsage.app"
EXEC="ClaudeUsage"

cd "$(dirname "$0")/.."

echo "==> Building ClaudeUsage (release)..."
swift build -c release

echo "==> Creating .app bundle..."
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
mkdir -p "$APP/Contents/Resources"

cp ".build/release/$EXEC" "$APP/Contents/MacOS/"
cp Resources/Info.plist "$APP/Contents/"

# Create entitlements for hardened runtime
ENTITLEMENTS=$(mktemp /tmp/entitlements.XXXXXX.plist)
cat > "$ENTITLEMENTS" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.app-sandbox</key>
    <false/>
    <key>com.apple.security.network.client</key>
    <true/>
    <key>com.apple.security.network.server</key>
    <true/>
</dict>
</plist>
PLIST

echo "==> Signing with: $IDENTITY"
codesign --force --options runtime \
    --sign "$IDENTITY" \
    --entitlements "$ENTITLEMENTS" \
    --timestamp \
    "$APP"

rm -f "$ENTITLEMENTS"

echo "==> Verifying signature..."
codesign --verify --deep --strict --verbose=2 "$APP" 2>&1

echo ""
echo "Binary size: $(du -sh "$APP/Contents/MacOS/$EXEC" | cut -f1)"
echo "App bundle:  $(du -sh "$APP" | cut -f1)"
echo ""
echo "To run:     open $APP"
echo "To install: cp -R $APP /Applications/"
echo ""
echo "To notarize:"
echo "  ditto -c -k --keepParent $APP ClaudeUsage.zip"
echo "  xcrun notarytool submit ClaudeUsage.zip --keychain-profile \"ClaudeUsage\" --wait"
echo "  xcrun stapler staple $APP"
