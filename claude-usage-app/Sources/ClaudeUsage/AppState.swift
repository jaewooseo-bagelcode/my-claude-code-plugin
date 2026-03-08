import AppKit
import Foundation
import os

private let logger = Logger(subsystem: "com.sugarscone.claude-usage", category: "AppState")

@Observable
final class AppState {
    var accounts: [Account] = []
    var menuBarOrgId: String {
        didSet { UserDefaults.standard.set(menuBarOrgId, forKey: "menuBarOrgId") }
    }
    private var pollTimer: Timer?
    private(set) var lastPollTime: Date?
    private(set) var lastPollError: String?
    private(set) var pollCount: Int = 0

    // Login flow state
    enum LoginStep { case idle, pickProfile, waitingForLogin, extracting }
    var loginStep: LoginStep = .idle
    var loginError: String?
    var availableProfiles: [OrionService.OrionProfile] = []
    private var loginTask: Task<Void, Never>?
    private var loginProfileUUID: String?

    let orion = OrionService.shared

    /// The account shown in the menu bar
    var menuBarAccount: Account? {
        accounts.first { $0.orgId == menuBarOrgId }
            ?? accounts.first
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

    init() {
        menuBarOrgId = UserDefaults.standard.string(forKey: "menuBarOrgId") ?? ""
        accounts = KeychainService.loadAccounts()
        debugLog("init: \(accounts.count) accounts")

        // Backfill empty emails from org name
        var changed = false
        for i in accounts.indices where accounts[i].email.isEmpty {
            let name = accounts[i].organizationName
            if let range = name.range(of: #"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}"#, options: .regularExpression) {
                accounts[i].email = String(name[range])
                changed = true
            }
        }
        if changed { try? KeychainService.saveAccounts(accounts) }

        // Ensure Orion profiles claude-usage-1..3 exist
        orion.ensureProfilesExist()

        startPolling()

        // Refresh on wake from sleep
        NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didWakeNotification,
            object: nil, queue: .main
        ) { [weak self] _ in
            self?.debugLog("WAKE — restarting poll in 10s")
            self?.startPolling(initialDelay: 10)
        }

        NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.willSleepNotification,
            object: nil, queue: .main
        ) { [weak self] _ in
            self?.debugLog("SLEEP — pollCount=\(self?.pollCount ?? 0)")
        }
    }

    // MARK: - Polling (ALL accounts via URLSession)

    func startPolling(initialDelay: TimeInterval = 3) {
        pollTimer?.invalidate()
        debugLog("startPolling: interval=300s, initialDelay=\(initialDelay)s")
        pollTimer = Timer.scheduledTimer(withTimeInterval: 300, repeats: true) { [weak self] _ in
            self?.refreshAll()
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + initialDelay) { [weak self] in
            self?.refreshAll()
        }
    }

    var isPollingHealthy: Bool {
        guard let last = lastPollTime else { return pollCount == 0 }
        return Date().timeIntervalSince(last) < 600
    }

    func ensurePollingAlive() {
        if !isPollingHealthy {
            debugLog("HEALTH CHECK FAILED — restarting")
            startPolling(initialDelay: 5)
        }
    }

    func refreshAll() {
        pollCount += 1
        lastPollTime = Date()
        lastPollError = nil
        debugLog("refreshAll #\(pollCount): \(accounts.count) accounts")

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
        guard let profileId = account.orionProfileId else {
            accounts[index].error = "No Orion profile linked"
            return
        }

        do {
            let usage = try await orion.fetchUsage(profileUUID: profileId, orgId: account.orgId)
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

    // MARK: - Login Flow (Orion Profile)

    func beginAddAccount() {
        loginError = nil
        let usedIds = Set(accounts.compactMap(\.orionProfileId))
        availableProfiles = orion.availableSlots(usedProfileIds: usedIds)

        if availableProfiles.isEmpty {
            loginError = accounts.count >= OrionService.maxSlots
                ? "Max \(OrionService.maxSlots) accounts reached"
                : "No Orion profiles found. Install Orion: brew install --cask orion"
            return
        }

        // If only one slot available, skip picker
        if availableProfiles.count == 1 {
            selectProfile(availableProfiles[0])
        } else {
            loginStep = .pickProfile
        }
    }

    func selectProfile(_ profile: OrionService.OrionProfile) {
        loginProfileUUID = profile.uuid
        loginStep = .waitingForLogin
        loginError = nil

        // Open profile browser to claude.ai
        orion.openProfile(profile.uuid, url: "https://claude.ai/settings/usage")

        loginTask = Task { @MainActor in
            do {
                // Wait for sessionKey cookie
                loginStep = .waitingForLogin
                try await orion.waitForLogin(profileUUID: profile.uuid, timeout: 300)

                // Extract organizations
                loginStep = .extracting
                let orgs = try await orion.extractOrganizations(profileUUID: profile.uuid)
                guard !orgs.isEmpty else { throw OrionError.apiFailed("No organizations found") }

                // Create or update accounts
                for info in orgs {
                    if let existing = accounts.firstIndex(where: { $0.orgId == info.orgId }) {
                        accounts[existing].email = info.email
                        accounts[existing].organizationName = info.orgName
                        accounts[existing].planType = info.planType
                        accounts[existing].orionProfileId = profile.uuid
                    } else {
                        accounts.append(Account(
                            id: UUID(),
                            orgId: info.orgId,
                            email: info.email,
                            organizationName: info.orgName,
                            planType: info.planType.isEmpty ? "claude" : info.planType,
                            orionProfileId: profile.uuid
                        ))
                    }
                }

                saveAccounts()
                logger.info("Added \(orgs.count) org(s) from profile \(profile.name)")

                refreshAll()
                loginStep = .idle
                loginProfileUUID = nil
                loginTask = nil
            } catch is CancellationError {
                // User cancelled
            } catch {
                loginError = error.localizedDescription
                loginStep = .idle
                loginProfileUUID = nil
                loginTask = nil
            }
        }
    }

    func cancelAddAccount() {
        loginTask?.cancel()
        loginTask = nil
        loginStep = .idle
        loginError = nil
        loginProfileUUID = nil
    }

    // MARK: - Show Browser

    func showBrowser(for account: Account) {
        guard let profileId = account.orionProfileId else { return }
        orion.openProfile(profileId, url: "https://claude.ai/settings/usage")
    }

    // MARK: - Remove

    func removeAccountGroup(email: String) {
        accounts.removeAll { $0.email == email }
        saveAccounts()
    }

    func removeAccount(_ account: Account) {
        accounts.removeAll { $0.id == account.id }
        saveAccounts()
    }

    // MARK: - CLI Login

    func claudeAuthLogin(for account: Account) {
        guard let profileId = account.orionProfileId else { return }
        let profiles = orion.discoverProfiles()
        guard let profile = profiles.first(where: { $0.uuid == profileId }) else { return }

        Task.detached {
            let helper = FileManager.default.temporaryDirectory.appendingPathComponent("open-orion-\(profileId).sh")
            let script = "#!/bin/bash\nopen -a \"\(profile.appPath)\" \"$@\"\n"
            try? script.write(to: helper, atomically: true, encoding: .utf8)
            try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: helper.path)

            let process = Process()
            let paths = [
                "\(FileManager.default.homeDirectoryForCurrentUser.path)/.local/bin/claude",
                "/usr/local/bin/claude",
                "/opt/homebrew/bin/claude",
            ]
            guard let claudePath = paths.first(where: { FileManager.default.fileExists(atPath: $0) }) else { return }
            process.executableURL = URL(fileURLWithPath: claudePath)
            process.arguments = ["auth", "login"]
            process.environment = ProcessInfo.processInfo.environment.merging(
                ["BROWSER": helper.path]
            ) { _, new in new }

            try? process.run()
            process.waitUntilExit()
            try? FileManager.default.removeItem(at: helper)
        }
    }

    private func saveAccounts() {
        try? KeychainService.saveAccounts(accounts)
    }

    // MARK: - Debug

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
