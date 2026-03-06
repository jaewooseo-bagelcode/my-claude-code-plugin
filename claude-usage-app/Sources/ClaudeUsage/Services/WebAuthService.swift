import AppKit
import Foundation
import WebKit
import os

private let logger = Logger(subsystem: "com.sugarscone.claude-usage", category: "WebAuth")

final class WebSession: NSObject, WKNavigationDelegate {
    let accountId: UUID
    let webView: WKWebView
    private var offscreenWindow: NSWindow?
    private var navigationContinuation: CheckedContinuation<Void, Never>?

    init(accountId: UUID) {
        self.accountId = accountId
        let dataStore = WKWebsiteDataStore(forIdentifier: accountId)
        let config = WKWebViewConfiguration()
        config.websiteDataStore = dataStore
        self.webView = WKWebView(frame: CGRect(x: 0, y: 0, width: 500, height: 700), configuration: config)
        super.init()
        self.webView.navigationDelegate = self

        let window = NSWindow(
            contentRect: NSRect(x: -10000, y: -10000, width: 500, height: 700),
            styleMask: [.borderless], backing: .buffered, defer: false
        )
        window.contentView = webView
        window.orderBack(nil)
        self.offscreenWindow = window
    }

    // MARK: - Step 1: Send code

    func sendCode(email: String) async throws {
        debugLog("sendCode START email=\(email)")

        // Load login page, wait for navigation
        webView.load(URLRequest(url: URL(string: "https://claude.ai/login")!))
        await waitForNav()

        // Wait for email input to be in DOM
        try await poll("if (document.querySelector('input#email')) return 'found';", timeout: 60)
        debugLog("sendCode: email input ready")

        // Fill email
        try await callJS("""
        const input = document.querySelector('input#email');
        input.focus();
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, email);
        input.dispatchEvent(new Event('input', {bubbles: true}));
        input.dispatchEvent(new Event('change', {bubbles: true}));
        """, args: ["email": email])

        // Click "이메일로 계속하기"
        try? await callJS("""
        const btn = Array.from(document.querySelectorAll('button')).find(b =>
            b.textContent.includes('이메일로 계속') || b.textContent.includes('Continue with email'));
        if (btn) btn.click();
        """, args: [:])
        debugLog("sendCode: clicked submit")

        // Poll for page change — dump body on each attempt for debugging
        var pollCount = 0
        let result = try await poll("""
        const body = document.body?.innerText || '';
        if (body.includes('전송된') || body.includes('sent')) return 'sent';
        if (body.includes('오류') || body.includes('error') || body.includes('Error')) return 'error';
        """, timeout: 60, onPoll: { body in
            pollCount += 1
            if pollCount <= 3 || pollCount % 5 == 0 {
                self.debugLog("sendCode poll[\(pollCount)] body=\(String(body.prefix(200)))")
            }
        })
        debugLog("sendCode: result=\(result)")

        if result.contains("error") {
            throw WebAuthError.apiFailed("Login failed — try again later")
        }

        // Click "인증 코드 입력" to show code input
        try? await callJS("""
        const btn = Array.from(document.querySelectorAll('button')).find(b =>
            b.textContent.includes('인증 코드') || b.textContent.includes('Enter code'));
        if (btn) btn.click();
        """, args: [:])

        // Poll for code input to appear
        try await poll("if (document.querySelector('input:not([type=email]):not([type=hidden])')) return 'found';", timeout: 60)
        debugLog("sendCode: code input ready")
    }

    // MARK: - Step 2: Verify code

    func verifyCode(_ code: String) async throws {
        debugLog("verifyCode START")

        // Fill code input
        let fillResult = try await callJS("""
        const inputs = Array.from(document.querySelectorAll('input'));
        const codeInput = inputs.find(inp => inp.type !== 'email' && inp.type !== 'hidden');
        if (!codeInput) return 'no_input';
        codeInput.focus();
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(codeInput, code);
        codeInput.dispatchEvent(new Event('input', {bubbles: true}));
        codeInput.dispatchEvent(new Event('change', {bubbles: true}));
        return 'filled: ' + codeInput.value;
        """, args: ["code": code])
        debugLog("verifyCode: fill=\(fillResult as? String ?? "nil")")

        if (fillResult as? String)?.hasPrefix("no_") == true {
            throw WebAuthError.apiFailed("Code input not found")
        }

        // Click verify/submit
        try? await callJS("""
        const btn = Array.from(document.querySelectorAll('button')).find(b => {
            const t = b.textContent;
            return (t.includes('인증') || t.includes('Verify') || t.includes('Continue')
                || t.includes('확인')) && !t.includes('Google') && !t.includes('이메일로');
        });
        if (btn) btn.click();
        """, args: [:])
        debugLog("verifyCode: clicked submit")

        // Poll for login completion or error
        let result = try await poll("""
        const body = document.body?.innerText || '';
        const url = location.href;
        if (!url.includes('/login')) return 'logged_in';
        if (body.includes('잘못된') || body.includes('invalid') || body.includes('만료') || body.includes('expired')) return 'invalid_code';
        """, timeout: 60)
        debugLog("verifyCode: result=\(result)")

        if result.contains("invalid") || result.contains("잘못") || result.contains("만료") {
            throw WebAuthError.apiFailed("Invalid or expired code")
        }
    }

    // MARK: - Step 3: Extract org info

    func extractAccountInfo() async throws -> (orgId: String, planType: String) {
        debugLog("extractAccountInfo START url=\(webView.url?.absoluteString ?? "")")

        // Navigate to settings/usage
        webView.load(URLRequest(url: URL(string: "https://claude.ai/settings/usage")!))

        // Wait for data-org-plan attribute (proves we're logged in and page rendered)
        try await poll("if (document.documentElement.dataset.orgPlan) return 'found';", timeout: 60)
        debugLog("extractAccountInfo: org-plan attribute found")

        // Extract org_id
        for attempt in 0..<5 {
            if attempt > 0 { try await Task.sleep(for: .seconds(2)) }

            let result = try? await callJS("""
            const entries = performance.getEntriesByType('resource');
            for (const e of entries) {
                const m = e.name.match(/organizations\\/([0-9a-f-]{36})/);
                if (m) return {orgId: m[1], plan: document.documentElement.dataset.orgPlan || ''};
            }
            const html = document.body?.innerHTML || '';
            const m2 = html.match(/organizations\\/([0-9a-f-]{36})/);
            if (m2) return {orgId: m2[1], plan: document.documentElement.dataset.orgPlan || ''};
            return null;
            """, args: [:]) as? [String: Any]

            if let orgId = result?["orgId"] as? String {
                let plan = result?["plan"] as? String ?? ""
                debugLog("extractAccountInfo: org=\(orgId) plan=\(plan)")
                return (orgId: orgId, planType: plan)
            }
        }
        throw WebAuthError.orgNotFound
    }

    // MARK: - Fetch usage

    func fetchUsage(orgId: String) async throws -> UsageResponse {
        if webView.url?.host != "claude.ai" {
            webView.load(URLRequest(url: URL(string: "https://claude.ai/settings/usage")!))
            try await poll("if (document.documentElement.dataset.orgPlan) return 'found';", timeout: 60)
        }

        let result = try await callJS("""
        const r = await fetch('/api/organizations/' + orgId + '/usage');
        if (!r.ok) return {_error: 'HTTP ' + r.status};
        return await r.json();
        """, args: ["orgId": orgId])

        guard let dict = result as? [String: Any] else { throw WebAuthError.invalidResponse }
        if let error = dict["_error"] as? String { throw WebAuthError.apiFailed(error) }
        let data = try JSONSerialization.data(withJSONObject: dict)
        return try JSONDecoder().decode(UsageResponse.self, from: data)
    }

    // MARK: - Helpers

    /// Poll from Swift side — each iteration is a fresh JS eval, survives page transitions.
    @discardableResult
    private func poll(_ conditionJS: String, timeout: Int, onPoll: ((String) -> Void)? = nil) async throws -> String {
        let deadline = Date().addingTimeInterval(TimeInterval(timeout))
        var lastError: String?
        while Date() < deadline {
            do {
                let result = try await callJS(conditionJS, args: [:])
                if let str = result as? String {
                    return str
                }
                // Dump body for debugging if callback provided
                if let onPoll {
                    let body = (try? await callJS("return document.body?.innerText || '';", args: [:])) as? String ?? ""
                    onPoll(body)
                }
            } catch {
                lastError = error.localizedDescription
            }
            try await Task.sleep(for: .milliseconds(500))
        }
        // Final dump
        let finalBody = (try? await callJS("return (document.body?.innerText || '').substring(0, 300);", args: [:])) as? String ?? "n/a"
        debugLog("poll timeout. lastError=\(lastError ?? "none") finalBody=\(finalBody)")
        throw WebAuthError.apiFailed("Timeout waiting for condition")
    }

    @discardableResult
    private func callJS(_ js: String, args: [String: Any]) async throws -> Any? {
        try await withCheckedThrowingContinuation { cont in
            webView.callAsyncJavaScript(js, arguments: args, in: nil, in: .page) { result in
                switch result {
                case .success(let value): cont.resume(returning: value)
                case .failure(let error): cont.resume(throwing: error)
                }
            }
        }
    }

    private func waitForNav() async {
        await withCheckedContinuation { cont in self.navigationContinuation = cont }
    }

    func webView(_ wv: WKWebView, didFinish navigation: WKNavigation!) {
        navigationContinuation?.resume()
        navigationContinuation = nil
    }

    private func debugLog(_ msg: String) {
        let logDir = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".cache/claude-usage")
        try? FileManager.default.createDirectory(at: logDir, withIntermediateDirectories: true)
        let file = logDir.appendingPathComponent("webauth.log")
        let ts = ISO8601DateFormatter().string(from: Date())
        let line = "[\(ts)] \(msg)\n"
        if let data = line.data(using: .utf8) {
            if FileManager.default.fileExists(atPath: file.path) {
                if let h = try? FileHandle(forWritingTo: file) { h.seekToEndOfFile(); h.write(data); h.closeFile() }
            } else { try? data.write(to: file) }
        }
    }
}

enum WebAuthError: LocalizedError {
    case orgNotFound, invalidResponse, apiFailed(String), cancelled

    var errorDescription: String? {
        switch self {
        case .orgNotFound: return "Could not detect organization"
        case .invalidResponse: return "Invalid API response"
        case .apiFailed(let msg): return msg
        case .cancelled: return "Cancelled"
        }
    }
}
