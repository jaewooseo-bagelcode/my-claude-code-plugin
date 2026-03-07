import AppKit
import Foundation
import os

private let logger = Logger(subsystem: "com.sugarscone.claude-usage", category: "AppState")

@Observable
final class AppState {
    var accounts: [Account] = []
    var activeEmail: String {
        didSet { UserDefaults.standard.set(activeEmail, forKey: "activeEmail") }
    }
    var menuBarOrgId: String {
        didSet { UserDefaults.standard.set(menuBarOrgId, forKey: "menuBarOrgId") }
    }
    var browsers: [UUID: BrowserAuthService] = [:]
    private var pollTimer: Timer?
    private(set) var lastPollTime: Date?
    private(set) var lastPollError: String?
    private(set) var pollCount: Int = 0

    // Login flow state
    enum LoginStep { case idle, waitingForLogin, extracting }
    var loginStep: LoginStep = .idle
    var loginError: String?
    private var loginService: BrowserAuthService?
    private var loginTask: Task<Void, Never>?

    /// Active accounts = orgs under the current Safari session email
    var activeAccounts: [Account] {
        accounts.filter { $0.email == activeEmail && !activeEmail.isEmpty }
    }

    /// Inactive = everything else, grouped by email
    var inactiveAccounts: [Account] {
        accounts.filter { $0.email != activeEmail || activeEmail.isEmpty }
    }

    var menuBarText: String {
        guard let p = menuBarAccount else { return "--·--" }
        let s = Int(p.fiveHour?.utilization ?? 0)
        let w = Int(p.sevenDay?.utilization ?? 0)
        return "\(s)·\(w)"
    }

    var statusColor: StatusColor {
        let util = menuBarAccount?.fiveHour?.utilization ?? 0
        if util >= 90 { return .red }
        if util >= 75 { return .yellow }
        return .normal
    }

    enum StatusColor { case normal, yellow, red }

    /// The account shown in the menu bar
    var menuBarAccount: Account? {
        accounts.first { $0.orgId == menuBarOrgId }
            ?? activeAccounts.first
    }

    init() {
        activeEmail = UserDefaults.standard.string(forKey: "activeEmail") ?? ""
        menuBarOrgId = UserDefaults.standard.string(forKey: "menuBarOrgId") ?? ""
        accounts = KeychainService.loadAccounts()
        debugLog("init: \(accounts.count) accounts, activeEmail=\(activeEmail)")

        // Backfill empty emails from org name (e.g. "user@example.com's Organization")
        var changed = false
        for i in accounts.indices where accounts[i].email.isEmpty {
            let name = accounts[i].organizationName
            if let range = name.range(of: #"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}"#, options: .regularExpression) {
                accounts[i].email = String(name[range])
                changed = true
            }
        }
        if changed { try? KeychainService.saveAccounts(accounts) }

        // Fallback: if saved email doesn't match any account, use first non-empty
        if !accounts.contains(where: { $0.email == activeEmail && !$0.email.isEmpty }) {
            activeEmail = accounts.first(where: { !$0.email.isEmpty })?.email ?? accounts.first?.email ?? ""
        }
        for account in accounts {
            browsers[account.id] = BrowserAuthService(accountId: account.id)
        }
        startPolling()

        // Refresh on wake from sleep — wait 10s for Safari to be ready
        NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didWakeNotification,
            object: nil, queue: .main
        ) { [weak self] _ in
            self?.debugLog("WAKE from sleep — will restart poll in 10s")
            self?.startPolling(initialDelay: 10)
        }

        // Sleep notification — log for diagnostics
        NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.willSleepNotification,
            object: nil, queue: .main
        ) { [weak self] _ in
            self?.debugLog("SLEEP — pollCount=\(self?.pollCount ?? 0)")
        }
    }

    // MARK: - Polling (active accounts only)

    func startPolling(initialDelay: TimeInterval = 3) {
        pollTimer?.invalidate()
        debugLog("startPolling: interval=300s, initialDelay=\(initialDelay)s")
        pollTimer = Timer.scheduledTimer(withTimeInterval: 300, repeats: true) { [weak self] _ in
            self?.refreshActive()
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + initialDelay) { [weak self] in
            self?.refreshActive()
        }
    }

    /// Check if polling is healthy — call periodically to detect stalled timers
    var isPollingHealthy: Bool {
        guard let last = lastPollTime else { return pollCount == 0 }
        return Date().timeIntervalSince(last) < 600 // <10 min since last poll
    }

    func ensurePollingAlive() {
        if !isPollingHealthy {
            debugLog("HEALTH CHECK FAILED — lastPoll=\(lastPollTime?.description ?? "never"), restarting")
            startPolling(initialDelay: 5)
        }
    }

    func refreshActive() {
        pollCount += 1
        lastPollTime = Date()
        lastPollError = nil
        let activeIndices = accounts.indices.filter { accounts[$0].email == activeEmail }
        debugLog("refreshActive #\(pollCount): \(activeIndices.count) accounts")

        for i in activeIndices {
            let index = i
            Task { @MainActor in
                await self.refreshAccount(at: index)
            }
        }
    }

    @MainActor
    func refreshAccount(at index: Int) async {
        guard accounts.indices.contains(index) else { return }
        let account = accounts[index]
        let service = browsers[account.id] ?? BrowserAuthService(accountId: account.id)
        browsers[account.id] = service

        do {
            let usage = try await service.fetchUsage(orgId: account.orgId)
            accounts[index].fiveHour = usage.fiveHour
            accounts[index].sevenDay = usage.sevenDay
            accounts[index].sevenDaySonnet = usage.sevenDaySonnet
            accounts[index].extraUsage = usage.extraUsage
            accounts[index].lastUpdated = Date()
            accounts[index].error = nil
        } catch {
            let msg = error.localizedDescription
            logger.error("refreshAccount[\(account.displayName)]: \(error)")
            debugLog("refreshAccount ERROR [\(account.displayName)]: \(msg)")
            accounts[index].error = msg
            lastPollError = msg
        }
    }

    // MARK: - Login Flow

    func beginAddAccount() {
        loginService = BrowserAuthService(accountId: UUID())
        loginStep = .extracting
        loginError = nil

        loginTask = Task { @MainActor in
            guard let service = loginService else { return }
            do {
                // Step 1: Try current Safari session first
                var orgs: [BrowserAuthService.AccountInfo] = []
                let currentOrgs = try? await service.extractAllOrganizations()
                let currentEmail = currentOrgs?.first?.email ?? ""

                if let found = currentOrgs, !found.isEmpty, currentEmail != activeEmail {
                    // Different account already logged in → use it directly
                    orgs = found
                } else {
                    // Same account or not logged in → logout and fresh login
                    if currentEmail == activeEmail && !activeEmail.isEmpty {
                        await service.logoutSafari()
                        try await Task.sleep(for: .seconds(1))
                    }

                    loginStep = .waitingForLogin
                    service.openLoginPage()
                    try await service.waitForLogin(timeout: 300)

                    loginStep = .extracting
                    orgs = try await service.extractAllOrganizations()
                }

                guard !orgs.isEmpty else { throw BrowserAuthError.orgNotFound }

                let email = orgs.first?.email ?? ""
                activeEmail = email

                // Create or update accounts for each org
                for info in orgs {
                    if let existing = accounts.firstIndex(where: { $0.orgId == info.orgId }) {
                        accounts[existing].email = info.email
                        accounts[existing].organizationName = info.orgName
                        accounts[existing].planType = info.planType
                    } else {
                        let account = Account(
                            id: UUID(),
                            orgId: info.orgId,
                            email: info.email,
                            organizationName: info.orgName,
                            planType: info.planType.isEmpty ? "claude" : info.planType
                        )
                        accounts.append(account)
                        browsers[account.id] = BrowserAuthService(accountId: account.id)
                    }
                }

                saveAccounts()
                logger.info("Added \(orgs.count) org(s) for \(email)")

                // Refresh active accounts
                refreshActive()

                loginStep = .idle
                loginService = nil
                loginTask = nil
            } catch is CancellationError {
                // User cancelled
            } catch {
                loginError = error.localizedDescription
                loginStep = .idle
                loginService = nil
                loginTask = nil
            }
        }
    }

    func cancelAddAccount() {
        loginTask?.cancel()
        loginTask = nil
        loginStep = .idle
        loginError = nil
        loginService = nil
    }

    // MARK: - Remove (by email group)

    func removeAccountGroup(email: String) {
        accounts.removeAll { $0.email == email }
        if activeEmail == email {
            activeEmail = accounts.first?.email ?? ""
        }
        saveAccounts()
    }

    func removeAccount(_ account: Account) {
        browsers.removeValue(forKey: account.id)
        accounts.removeAll { $0.id == account.id }
        saveAccounts()
    }

    private func saveAccounts() {
        try? KeychainService.saveAccounts(accounts)
    }

    // MARK: - Debug Log

    func debugLog(_ msg: String) {
        let logDir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".cache/claude-usage")
        try? FileManager.default.createDirectory(at: logDir, withIntermediateDirectories: true)
        let file = logDir.appendingPathComponent("app-state.log")
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
