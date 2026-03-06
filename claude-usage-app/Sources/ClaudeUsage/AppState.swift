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
    var browsers: [UUID: BrowserAuthService] = [:]
    private var pollTimer: Timer?

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

    /// First active account drives the menu bar
    var menuBarText: String {
        guard let p = activeAccounts.first else { return "--·--" }
        let s = Int(p.fiveHour?.utilization ?? 0)
        let w = Int(p.sevenDay?.utilization ?? 0)
        return "\(s)·\(w)"
    }

    var statusColor: StatusColor {
        let util = activeAccounts.compactMap { $0.fiveHour?.utilization }.max() ?? 0
        if util >= 90 { return .red }
        if util >= 75 { return .yellow }
        return .normal
    }

    enum StatusColor { case normal, yellow, red }

    init() {
        activeEmail = UserDefaults.standard.string(forKey: "activeEmail") ?? ""
        accounts = KeychainService.loadAccounts()

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
    }

    // MARK: - Polling (active accounts only)

    func startPolling() {
        pollTimer?.invalidate()
        pollTimer = Timer.scheduledTimer(withTimeInterval: 300, repeats: true) { [weak self] _ in
            self?.refreshActive()
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 3) { [weak self] in
            self?.refreshActive()
        }
    }

    func refreshActive() {
        for i in accounts.indices where accounts[i].email == activeEmail {
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
            logger.error("refreshAccount[\(account.displayName)]: \(error)")
            accounts[index].error = error.localizedDescription
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
}
