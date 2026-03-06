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
    private func runAppleScript(_ source: String, timeout: TimeInterval = 30) async throws -> String {
        try await withThrowingTaskGroup(of: String.self) { group in
            group.addTask {
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
            group.addTask {
                try await Task.sleep(for: .seconds(timeout))
                throw BrowserAuthError.commandFailed("AppleScript timed out after \(Int(timeout))s")
            }
            let result = try await group.next()!
            group.cancelAll()
            return result
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

    /// Execute JS in any claude.ai tab (finds one, or opens a hidden tab)
    @discardableResult
    private func safariJSClaudeTab(_ js: String) async throws -> String {
        let escaped = escapeJS(js)
        return try await runAppleScript("""
        tell application "Safari"
            set foundTab to missing value
            if (count of windows) > 0 then
                repeat with w in windows
                    repeat with t in tabs of w
                        try
                            if URL of t starts with "https://claude.ai" then
                                set foundTab to t
                                exit repeat
                            end if
                        end try
                    end repeat
                    if foundTab is not missing value then exit repeat
                end repeat
            end if

            if foundTab is missing value then
                -- Create a hidden window for background API calls
                set newDoc to make new document with properties {URL:"https://claude.ai/settings"}
                set visible of front window to false
                -- Wait up to 15s for page to load
                repeat 15 times
                    delay 1
                    try
                        set pageURL to URL of current tab of front window
                        if pageURL starts with "https://claude.ai" then exit repeat
                    end try
                end repeat
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

    /// Extract ALL organizations from the current Safari session.
    /// One login can have multiple orgs (e.g. Personal + Enterprise).
    func extractAllOrganizations() async throws -> [AccountInfo] {
        debugLog("extractAllOrganizations START")

        // Navigate to settings if needed
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

        // Fetch all organizations
        let orgsJS = "var x=new XMLHttpRequest();x.open('GET','/api/organizations',false);x.send();x.responseText"

        var orgsData: [[String: Any]] = []
        for attempt in 0..<15 {
            let result: String
            do {
                result = try await safariJSCurrentTab(orgsJS)
            } catch {
                debugLog("extractAllOrgs[\(attempt)] JS ERROR: \(error)")
                try await Task.sleep(for: .seconds(2))
                continue
            }

            if let data = result.data(using: .utf8),
               let parsed = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]],
               !parsed.isEmpty
            {
                orgsData = parsed
                debugLog("extractAllOrgs: found \(parsed.count) orgs")
                break
            }

            try await Task.sleep(for: .seconds(2))
        }

        guard !orgsData.isEmpty else { throw BrowserAuthError.orgNotFound }

        // Get email from current account (try multiple API fields)
        var email = ""
        let emailJS = "var x=new XMLHttpRequest();x.open('GET','/api/auth/current_account',false);x.send();x.responseText"
        if let emailResult = try? await safariJSCurrentTab(emailJS),
           let emailData = emailResult.data(using: .utf8),
           let account = try? JSONSerialization.jsonObject(with: emailData) as? [String: Any]
        {
            email = account["email_address"] as? String
                ?? account["email"] as? String
                ?? account["primary_email"] as? String
                ?? ""
            debugLog("extractAllOrgs: email from current_account=\(email) keys=\(Array(account.keys))")
        }

        // Fallback: extract email from org name (e.g. "user@example.com's Organization")
        if email.isEmpty {
            for org in orgsData {
                let name = org["name"] as? String ?? ""
                if let range = name.range(of: #"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}"#, options: .regularExpression) {
                    email = String(name[range])
                    debugLog("extractAllOrgs: email from org name=\(email)")
                    break
                }
            }
        }

        // Build AccountInfo for each org that has chat capability
        var results: [AccountInfo] = []
        for org in orgsData {
            guard let orgId = org["uuid"] as? String, !orgId.isEmpty else { continue }

            // Skip API-only orgs (no chat = no usage to monitor)
            let capabilities = org["capabilities"] as? [String] ?? []
            guard capabilities.contains("chat") else {
                debugLog("extractAllOrgs: skip API-only org \(orgId)")
                continue
            }

            let name = org["name"] as? String ?? ""
            var plan = ""
            if capabilities.contains("raven_enterprise") { plan = "enterprise" }
            else if capabilities.contains("claude_max") { plan = "claude_max" }
            else if capabilities.contains("raven") { plan = "pro" }
            else { plan = "free" }

            debugLog("extractAllOrgs: org=\(orgId) name=\(name) plan=\(plan)")
            results.append(AccountInfo(orgId: orgId, planType: plan, email: email, orgName: name))
        }

        return results
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

    /// Logout from claude.ai in Safari so next Add Account gets a fresh login.
    /// Clears cookies via JS then navigates to logout URL and waits for login page.
    func logoutSafari() async {
        debugLog("logoutSafari START")

        // Method 1: POST to logout endpoint
        _ = try? await safariJSClaudeTab(
            "var x=new XMLHttpRequest();x.open('POST','/api/auth/logout',false);x.send();x.status"
        )

        // Method 2: Navigate to logout URL directly
        try? await runAppleScript("""
        tell application "Safari"
            set URL of current tab of front window to "https://claude.ai/api/auth/logout"
        end tell
        """)

        try? await Task.sleep(for: .seconds(2))

        // Navigate to login page
        try? await runAppleScript("""
        tell application "Safari"
            set URL of current tab of front window to "https://claude.ai/login"
        end tell
        """)

        // Wait until we're actually on the login page (not auto-redirected back)
        for _ in 0..<15 {
            try? await Task.sleep(for: .seconds(1))
            let url = (try? await runAppleScript("""
            tell application "Safari"
                URL of current tab of front window
            end tell
            """)) ?? ""
            debugLog("logoutSafari: url=\(url)")
            if url.contains("/login") {
                debugLog("logoutSafari: SUCCESS — on login page")
                return
            }
        }
        debugLog("logoutSafari: may not have fully logged out")
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
