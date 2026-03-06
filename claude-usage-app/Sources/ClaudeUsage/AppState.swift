import AppKit
import Foundation
import os

private let logger = Logger(subsystem: "com.sugarscone.claude-usage", category: "AppState")

@Observable
final class AppState {
    var accounts: [Account] = []
    var activeAccountId: UUID?
    var browsers: [UUID: BrowserAuthService] = [:]
    private var pollTimer: Timer?

    // Login flow state
    enum LoginStep { case idle, waitingForLogin, extracting }
    var loginStep: LoginStep = .idle
    var loginError: String?
    private var loginService: BrowserAuthService?
    private var loginAccountId: UUID?
    private var loginTask: Task<Void, Never>?

    var activeAccount: Account? {
        accounts.first { $0.id == activeAccountId }
    }

    var inactiveAccounts: [Account] {
        accounts.filter { $0.id != activeAccountId }
    }

    var menuBarText: String {
        guard let p = activeAccount else { return "--·--" }
        let s = Int(p.fiveHour?.utilization ?? 0)
        let w = Int(p.sevenDay?.utilization ?? 0)
        return "\(s)·\(w)"
    }

    var statusColor: StatusColor {
        let util = activeAccount?.fiveHour?.utilization ?? 0
        if util >= 90 { return .red }
        if util >= 75 { return .yellow }
        return .normal
    }

    enum StatusColor { case normal, yellow, red }

    init() {
        accounts = KeychainService.loadAccounts()
        // Set last account as active (most recently added)
        activeAccountId = accounts.last?.id
        for account in accounts {
            browsers[account.id] = BrowserAuthService(accountId: account.id)
        }
        startPolling()
    }

    // MARK: - Polling (only active account)

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
        guard let idx = accounts.firstIndex(where: { $0.id == activeAccountId }) else { return }
        Task { @MainActor in
            await refreshAccount(at: idx)
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
            logger.error("refreshAccount[\(account.email)]: \(error)")
            accounts[index].error = error.localizedDescription
        }
    }

    // MARK: - Login Flow

    func beginAddAccount() {
        let id = UUID()
        loginAccountId = id
        loginService = BrowserAuthService(accountId: id)
        loginStep = .waitingForLogin
        loginError = nil

        loginTask = Task { @MainActor in
            guard let service = loginService else { return }
            do {
                service.openLoginPage()

                try await service.waitForLogin(timeout: 300)

                loginStep = .extracting
                let info = try await service.extractAccountInfo()

                // Create or update account
                let accountId: UUID
                if let existing = accounts.firstIndex(where: { $0.orgId == info.orgId }) {
                    accountId = accounts[existing].id
                    accounts[existing].email = info.email
                    accounts[existing].organizationName = info.orgName
                    accounts[existing].planType = info.planType
                    browsers[accounts[existing].id] = service
                } else {
                    accountId = id
                    let account = Account(
                        id: id,
                        orgId: info.orgId,
                        email: info.email,
                        organizationName: info.orgName,
                        planType: info.planType.isEmpty ? "claude" : info.planType
                    )
                    accounts.append(account)
                    browsers[id] = service
                }

                // Set as active and refresh
                activeAccountId = accountId
                saveAccounts()

                if let idx = accounts.firstIndex(where: { $0.id == accountId }) {
                    await refreshAccount(at: idx)
                }

                // Logout Safari so next Add Account gets fresh login
                await service.logoutSafari()

                loginStep = .idle
                loginService = nil
                loginAccountId = nil
                loginTask = nil
                logger.info("Account added: \(info.orgId)")
            } catch is CancellationError {
                // User cancelled
            } catch {
                loginError = error.localizedDescription
                loginStep = .idle
                loginService = nil
                loginAccountId = nil
                loginTask = nil
            }
        }
    }

    func cancelAddAccount() {
        loginTask?.cancel()
        loginTask = nil
        if let service = loginService {
            Task { await service.closeBrowser() }
        }
        loginStep = .idle
        loginError = nil
        loginService = nil
        loginAccountId = nil
    }

    // MARK: - Remove

    func removeAccount(_ account: Account) {
        if let service = browsers[account.id] {
            Task { await service.clearSession() }
        }
        browsers.removeValue(forKey: account.id)
        accounts.removeAll { $0.id == account.id }
        if activeAccountId == account.id {
            activeAccountId = accounts.last?.id
        }
        saveAccounts()
    }

    private func saveAccounts() {
        try? KeychainService.saveAccounts(accounts)
    }
}
