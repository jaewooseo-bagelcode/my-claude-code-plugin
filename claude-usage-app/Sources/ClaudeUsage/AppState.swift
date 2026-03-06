import AppKit
import Foundation
import os

private let logger = Logger(subsystem: "com.sugarscone.claude-usage", category: "AppState")

@Observable
final class AppState {
    var accounts: [Account] = []
    var browsers: [UUID: BrowserAuthService] = [:]
    private var pollTimer: Timer?

    // Login flow state
    enum LoginStep { case idle, waitingForLogin, extracting }
    var loginStep: LoginStep = .idle
    var loginError: String?
    private var loginService: BrowserAuthService?
    private var loginAccountId: UUID?
    private var loginTask: Task<Void, Never>?

    var primaryAccount: Account? { accounts.first }

    var menuBarText: String {
        guard let p = primaryAccount else { return "--·--" }
        let s = Int(p.fiveHour?.utilization ?? 0)
        let w = Int(p.sevenDay?.utilization ?? 0)
        return "\(s)·\(w)"
    }

    var statusColor: StatusColor {
        let maxUtil = accounts.compactMap { $0.fiveHour?.utilization }.max() ?? 0
        if maxUtil >= 90 { return .red }
        if maxUtil >= 75 { return .yellow }
        return .normal
    }

    enum StatusColor { case normal, yellow, red }

    init() {
        accounts = KeychainService.loadAccounts()
        for account in accounts {
            browsers[account.id] = BrowserAuthService(accountId: account.id)
        }
        startPolling()
    }

    // MARK: - Polling

    func startPolling() {
        pollTimer?.invalidate()
        pollTimer = Timer.scheduledTimer(withTimeInterval: 300, repeats: true) { [weak self] _ in
            self?.refreshAll()
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 3) { [weak self] in
            self?.refreshAll()
        }
    }

    func refreshAll() {
        for i in accounts.indices {
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
            accounts[index].error = error.localizedDescription
        }
    }

    // MARK: - Login Flow (user logs in via headed browser)

    func beginAddAccount() {
        let id = UUID()
        loginAccountId = id
        loginService = BrowserAuthService(accountId: id)
        loginStep = .waitingForLogin
        loginError = nil

        loginTask = Task { @MainActor in
            guard let service = loginService else { return }
            do {
                // Open Safari — user handles Cloudflare + login
                service.openLoginPage()

                // Poll until login completes (5 min timeout)
                try await service.waitForLogin(timeout: 300)

                // Extract org info
                loginStep = .extracting
                let info = try await service.extractAccountInfo()

                // Create or update account
                if let existing = accounts.firstIndex(where: { $0.orgId == info.orgId }) {
                    accounts[existing].email = info.email
                    accounts[existing].planType = info.planType
                    browsers[accounts[existing].id] = service
                    saveAccounts()
                    await service.closeBrowser()
                    await refreshAccount(at: existing)
                } else {
                    let account = Account(
                        id: id,
                        orgId: info.orgId,
                        email: info.email,
                        organizationName: "",
                        planType: info.planType.isEmpty ? "claude" : info.planType
                    )
                    accounts.append(account)
                    browsers[id] = service
                    saveAccounts()
                    await service.closeBrowser()
                    await refreshAccount(at: accounts.count - 1)
                }

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
        saveAccounts()
    }

    private func saveAccounts() {
        try? KeychainService.saveAccounts(accounts)
    }
}
