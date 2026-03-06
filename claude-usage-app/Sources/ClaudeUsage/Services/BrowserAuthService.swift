import AppKit
import Foundation
import os

private let logger = Logger(subsystem: "com.sugarscone.claude-usage", category: "BrowserAuth")

/// Uses Safari + AppleScript to authenticate and fetch usage data.
/// Safari is the user's real browser — Cloudflare trusts it.
final class BrowserAuthService: @unchecked Sendable {
    let accountId: UUID

    init(accountId: UUID) {
        self.accountId = accountId
    }

    // MARK: - AppleScript helpers

    @discardableResult
    private func runAppleScript(_ source: String) async throws -> String {
        try await withCheckedThrowingContinuation { cont in
            DispatchQueue.global().async {
                var error: NSDictionary?
                guard let script = NSAppleScript(source: source) else {
                    cont.resume(throwing: BrowserAuthError.commandFailed("Invalid AppleScript"))
                    return
                }
                let result = script.executeAndReturnError(&error)
                if let error {
                    let msg = error[NSAppleScript.errorMessage] as? String ?? "AppleScript error"
                    cont.resume(throwing: BrowserAuthError.commandFailed(msg))
                } else {
                    cont.resume(returning: result.stringValue ?? "")
                }
            }
        }
    }

    private func escapeJS(_ js: String) -> String {
        js.replacingOccurrences(of: "\\", with: "\\\\")
           .replacingOccurrences(of: "\"", with: "\\\"")
           .replacingOccurrences(of: "\n", with: " ")
           .replacingOccurrences(of: "\r", with: " ")
    }

    /// Execute JS in Safari's current front tab
    @discardableResult
    private func safariJSCurrentTab(_ js: String) async throws -> String {
        let escaped = escapeJS(js)
        return try await runAppleScript("""
        tell application "Safari"
            do JavaScript "\(escaped)" in current tab of front window
        end tell
        """)
    }

    /// Execute JS in any claude.ai tab (finds one, or opens a new tab)
    @discardableResult
    private func safariJSClaudeTab(_ js: String) async throws -> String {
        let escaped = escapeJS(js)
        return try await runAppleScript("""
        tell application "Safari"
            set foundTab to missing value
            if (count of windows) > 0 then
                repeat with w in windows
                    repeat with t in tabs of w
                        if URL of t starts with "https://claude.ai" then
                            set foundTab to t
                            exit repeat
                        end if
                    end repeat
                    if foundTab is not missing value then exit repeat
                end repeat
            end if

            if foundTab is missing value then
                if (count of windows) = 0 then
                    make new document with properties {URL:"https://claude.ai/settings"}
                else
                    tell front window
                        set current tab to (make new tab with properties {URL:"https://claude.ai/settings"})
                    end tell
                end if
                delay 5
                set foundTab to current tab of front window
            end if

            do JavaScript "\(escaped)" in foundTab
        end tell
        """)
    }

    // MARK: - Login: open Safari and let user handle it

    func openLoginPage() {
        // Open settings/usage directly:
        // - Already logged in → lands on settings page → instant org extraction
        // - Not logged in → claude.ai redirects to /login → user logs in
        let url = URL(string: "https://claude.ai/settings/usage")!
        let safariURL = URL(fileURLWithPath: "/Applications/Safari.app")
        let config = NSWorkspace.OpenConfiguration()
        NSWorkspace.shared.open([url], withApplicationAt: safariURL, configuration: config)
    }

    /// Poll Safari tabs until we find a logged-in claude.ai page.
    /// Checks ALL tabs (not just front) so it detects already-logged-in sessions.
    func waitForLogin(timeout: TimeInterval = 300) async throws {
        debugLog("waitForLogin START")
        let deadline = Date().addingTimeInterval(timeout)

        while Date() < deadline {
            try Task.checkCancellation()

            // Get URLs of all Safari tabs
            let allUrls: String
            do {
                allUrls = try await runAppleScript("""
                tell application "Safari"
                    set urlList to {}
                    repeat with w in windows
                        repeat with t in tabs of w
                            set end of urlList to URL of t
                        end repeat
                    end repeat
                    set AppleScript's text item delimiters to linefeed
                    return urlList as text
                end tell
                """)
            } catch {
                debugLog("waitForLogin: can't get URLs: \(error), retrying...")
                try await Task.sleep(for: .seconds(2))
                continue
            }

            for urlStr in allUrls.components(separatedBy: "\n") {
                guard let parsed = URL(string: urlStr),
                      parsed.host?.hasSuffix("claude.ai") == true,
                      !parsed.path.contains("login"),
                      !parsed.path.contains("challenge")
                else { continue }

                debugLog("waitForLogin: SUCCESS url=\(urlStr)")
                return
            }

            debugLog("waitForLogin: not yet (\(allUrls.components(separatedBy: "\n").count) tabs checked)")
            try await Task.sleep(for: .seconds(2))
        }

        throw BrowserAuthError.timeout
    }

    // MARK: - Extract org info (from Safari tab after login)

    struct AccountInfo {
        let orgId: String
        let planType: String
        let email: String
        let orgName: String
    }

    func extractAccountInfo() async throws -> AccountInfo {
        debugLog("extractAccountInfo START")

        // Check if any Safari tab is already on settings/usage; if not, navigate
        let currentUrl = (try? await runAppleScript("""
        tell application "Safari"
            URL of current tab of front window
        end tell
        """)) ?? ""

        if !currentUrl.contains("claude.ai/settings") {
            try await runAppleScript("""
            tell application "Safari"
                set URL of current tab of front window to "https://claude.ai/settings/usage"
            end tell
            """)
        }

        try await Task.sleep(for: .seconds(4))

        // Step 1: Extract orgId from performance entries or HTML
        let orgJS = """
        (function() {
            var entries = performance.getEntriesByType('resource');
            for (var i = 0; i < entries.length; i++) {
                var m = entries[i].name.match(/organizations\\/([0-9a-f-]{36})/);
                if (m) return JSON.stringify({orgId: m[1], plan: document.documentElement.dataset.orgPlan || ''});
            }
            var html = document.body ? document.body.innerHTML : '';
            var m2 = html.match(/organizations\\/([0-9a-f-]{36})/);
            if (m2) return JSON.stringify({orgId: m2[1], plan: document.documentElement.dataset.orgPlan || ''});
            return '';
        })()
        """

        var orgId = ""
        var planType = ""

        for attempt in 0..<15 {
            let result: String
            do {
                result = try await safariJSCurrentTab(orgJS)
            } catch {
                debugLog("extractAccountInfo[\(attempt)] JS ERROR: \(error)")
                try await Task.sleep(for: .seconds(2))
                continue
            }
            debugLog("extractAccountInfo[\(attempt)] result=\(result)")

            if !result.isEmpty,
               let data = result.data(using: .utf8),
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: String],
               let id = json["orgId"], !id.isEmpty
            {
                orgId = id
                planType = json["plan"] ?? ""
                break
            }

            try await Task.sleep(for: .seconds(2))
        }

        guard !orgId.isEmpty else { throw BrowserAuthError.orgNotFound }

        // Step 2: Fetch org details (name, email) via API
        let detailJS = "var x=new XMLHttpRequest();x.open('GET','/api/organizations',false);x.send();x.responseText"
        var email = ""
        var orgName = ""

        if let detailResult = try? await safariJSCurrentTab(detailJS),
           let detailData = detailResult.data(using: .utf8),
           let orgs = try? JSONSerialization.jsonObject(with: detailData) as? [[String: Any]]
        {
            debugLog("extractAccountInfo: orgs count=\(orgs.count)")
            for org in orgs {
                if let id = org["uuid"] as? String, id == orgId {
                    orgName = org["name"] as? String ?? ""
                    // email might be in settings or join_token
                    if let settings = org["settings"] as? [String: Any] {
                        email = settings["primary_email"] as? String ?? ""
                    }
                    break
                }
            }
        }

        // Step 3: Try to get email from account info if not found
        if email.isEmpty {
            let emailJS = "var x=new XMLHttpRequest();x.open('GET','/api/auth/current_account',false);x.send();x.responseText"
            if let emailResult = try? await safariJSCurrentTab(emailJS),
               let emailData = emailResult.data(using: .utf8),
               let account = try? JSONSerialization.jsonObject(with: emailData) as? [String: Any]
            {
                email = account["email_address"] as? String
                    ?? account["email"] as? String
                    ?? ""
                debugLog("extractAccountInfo: email from account=\(email)")
            }
        }

        debugLog("extractAccountInfo: org=\(orgId) name=\(orgName) email=\(email) plan=\(planType)")
        return AccountInfo(orgId: orgId, planType: planType, email: email, orgName: orgName)
    }

    // MARK: - Fetch usage (via synchronous XHR in Safari)

    func fetchUsage(orgId: String) async throws -> UsageResponse {
        let js = "var x=new XMLHttpRequest();x.open('GET','/api/organizations/\(orgId)/usage',false);x.send();x.responseText"

        let result = try await safariJSClaudeTab(js)
        debugLog("fetchUsage: \(result.prefix(200))")

        guard let data = result.data(using: .utf8) else {
            throw BrowserAuthError.invalidResponse
        }

        do {
            return try JSONDecoder().decode(UsageResponse.self, from: data)
        } catch {
            debugLog("fetchUsage DECODE ERROR: \(error)")
            debugLog("fetchUsage RAW: \(result.prefix(500))")
            throw error
        }
    }

    // MARK: - Cleanup

    func closeBrowser() async {
        // No-op — Safari is the user's browser, we don't close it
    }

    /// Logout from claude.ai in Safari so next Add Account gets a fresh login
    func logoutSafari() async {
        debugLog("logoutSafari")
        _ = try? await safariJSCurrentTab(
            "fetch('/api/auth/logout',{method:'POST',credentials:'include'})"
        )
        try? await Task.sleep(for: .seconds(1))
        // Navigate to login page so it's ready for next add
        try? await runAppleScript("""
        tell application "Safari"
            set URL of current tab of front window to "https://claude.ai/login"
        end tell
        """)
    }

    func clearSession() async {
        // No-op — Safari manages its own sessions
    }

    // MARK: - Debug

    private func debugLog(_ msg: String) {
        let logDir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".cache/claude-usage")
        try? FileManager.default.createDirectory(at: logDir, withIntermediateDirectories: true)
        let file = logDir.appendingPathComponent("browser-auth.log")
        let ts = ISO8601DateFormatter().string(from: Date())
        let line = "[\(ts)] \(msg)\n"
        if let data = line.data(using: .utf8) {
            if FileManager.default.fileExists(atPath: file.path) {
                if let h = try? FileHandle(forWritingTo: file) {
                    h.seekToEndOfFile(); h.write(data); h.closeFile()
                }
            } else {
                try? data.write(to: file)
            }
        }
    }
}

enum BrowserAuthError: LocalizedError {
    case timeout
    case commandFailed(String)
    case loginFailed(String)
    case orgNotFound
    case invalidResponse

    var errorDescription: String? {
        switch self {
        case .timeout: return "Timed out waiting for login"
        case .commandFailed(let msg): return msg
        case .loginFailed(let msg): return msg
        case .orgNotFound: return "Could not detect organization"
        case .invalidResponse: return "Invalid API response"
        }
    }
}
