import SwiftUI

struct UsagePopoverView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Header
            HStack(spacing: 8) {
                Text("Claude Usage")
                    .font(.headline)
                Spacer()

                if appState.loginStep == .idle {
                    Button { appState.beginAddAccount() } label: {
                        Image(systemName: "plus.circle")
                    }
                    .buttonStyle(.borderless)
                    .help("Add Account")
                } else {
                    Button { appState.cancelAddAccount() } label: {
                        Image(systemName: "xmark.circle")
                    }
                    .buttonStyle(.borderless)
                    .help("Cancel")
                }

                Button { claudeAuthLogin() } label: {
                    Image(systemName: "terminal")
                }
                .buttonStyle(.borderless)
                .help("Claude CLI Login via Safari")

                Button { appState.refreshActive() } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .buttonStyle(.borderless)
                .help("Refresh")

                Button { NSApplication.shared.terminate(nil) } label: {
                    Image(systemName: "xmark")
                        .font(.caption)
                }
                .buttonStyle(.borderless)
                .foregroundStyle(.tertiary)
                .help("Quit")
            }
            .padding(.bottom, 8)

            Divider()

            // Login status
            if appState.loginStep != .idle {
                LoginStatusView()
                    .environment(appState)
                Divider()
            }

            if appState.accounts.isEmpty && appState.loginStep == .idle {
                VStack(spacing: 6) {
                    Image(systemName: "person.crop.circle.badge.plus")
                        .font(.title2)
                        .foregroundStyle(.secondary)
                    Text("No accounts")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 16)
            }

            // Active account group (full detail)
            if !appState.activeAccounts.isEmpty {
                AccountGroupView(
                    email: appState.activeEmail,
                    accounts: appState.activeAccounts,
                    isActive: true
                ) {
                    appState.removeAccountGroup(email: appState.activeEmail)
                }
                .padding(.vertical, 6)
            }

            // Inactive account groups (compact)
            let inactiveGroups = Dictionary(grouping: appState.inactiveAccounts, by: \.email)
            ForEach(inactiveGroups.keys.sorted(), id: \.self) { email in
                if let group = inactiveGroups[email] {
                    Divider()
                    AccountGroupView(
                        email: email,
                        accounts: group,
                        isActive: false
                    ) {
                        appState.removeAccountGroup(email: email)
                    }
                    .padding(.vertical, 4)
                }
            }
        }
        .padding()
        .frame(width: 320)
    }
}

// MARK: - Account Group (email → multiple orgs)

struct AccountGroupView: View {
    let email: String
    let accounts: [Account]
    let isActive: Bool
    let onRemoveGroup: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: isActive ? 8 : 4) {
            // Group header
            HStack {
                Text(email.isEmpty ? "Unknown" : email)
                    .font(.system(isActive ? .subheadline : .caption, design: .rounded))
                    .foregroundStyle(isActive ? .primary : .secondary)
                    .lineLimit(1)

                Spacer()

                Menu {
                    Button("Remove All", role: .destructive) { onRemoveGroup() }
                } label: {
                    Image(systemName: "ellipsis.circle")
                        .font(isActive ? .body : .caption2)
                        .foregroundStyle(isActive ? .secondary : .tertiary)
                }
                .menuIndicator(.hidden)
                .fixedSize()
            }

            // Org rows
            ForEach(accounts) { account in
                if isActive {
                    ActiveOrgRow(account: account)
                } else {
                    InactiveOrgRow(account: account)
                }
            }
        }
        .opacity(isActive ? 1.0 : 0.7)
    }
}

// MARK: - Active Org Row (full bars)

struct ActiveOrgRow: View {
    @Environment(AppState.self) private var appState
    let account: Account

    private var isMenuBarSource: Bool {
        appState.menuBarAccount?.orgId == account.orgId
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            // Org name + plan badge + pin
            HStack(spacing: 6) {
                Button {
                    appState.menuBarOrgId = account.orgId
                } label: {
                    Image(systemName: isMenuBarSource ? "chart.bar.fill" : "chart.bar")
                        .font(.caption)
                        .foregroundStyle(isMenuBarSource ? .blue : .gray.opacity(0.4))
                }
                .buttonStyle(.borderless)
                .help("Show in menu bar")

                Text(account.organizationName.isEmpty ? account.planType.capitalized : account.organizationName)
                    .font(.system(.headline, design: .rounded))
                    .lineLimit(1)

                Text(account.planType.capitalized)
                    .font(.caption2.bold())
                    .padding(.horizontal, 5)
                    .padding(.vertical, 1)
                    .background(.blue.opacity(0.2))
                    .foregroundStyle(.blue)
                    .clipShape(Capsule())

                Spacer()

                if account.error != nil {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.yellow)
                        .help(account.error ?? "")
                }
            }

            // Usage bars
            if let fiveHour = account.fiveHour {
                UsageBarView(icon: "bolt.fill", label: "Session",
                             value: fiveHour.utilization, resetDate: fiveHour.resetDate)
                    .saturation(isMenuBarSource ? 1.0 : 0.3)
            }
            if let sevenDay = account.sevenDay {
                UsageBarView(icon: "calendar", label: "Weekly",
                             value: sevenDay.utilization, resetDate: sevenDay.resetDate)
                    .saturation(isMenuBarSource ? 1.0 : 0.3)
            }
            if let extra = account.extraUsage, extra.isEnabled {
                ExtraUsageBarView(extra: extra)
                    .saturation(isMenuBarSource ? 1.0 : 0.3)
            }

            if account.fiveHour == nil && account.sevenDay == nil && account.error == nil {
                HStack {
                    ProgressView().controlSize(.small)
                    Text("Loading...").font(.caption).foregroundStyle(.tertiary)
                }
            }

            if let lastUpdated = account.lastUpdated {
                Text("Updated \(lastUpdated, style: .relative) ago")
                    .font(.caption2).foregroundStyle(.tertiary)
            }
        }
    }
}

// MARK: - Inactive Org Row (compact one-liner)

struct InactiveOrgRow: View {
    let account: Account

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                Text(account.organizationName.isEmpty ? account.planType.capitalized : account.organizationName)
                    .font(.system(.caption, design: .rounded))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)

                Text(account.planType.capitalized)
                    .font(.system(size: 9).bold())
                    .padding(.horizontal, 4)
                    .padding(.vertical, 1)
                    .background(.primary.opacity(0.08))
                    .foregroundStyle(.tertiary)
                    .clipShape(Capsule())

                Spacer()
            }

            // Session + Weekly summary
            HStack(spacing: 12) {
                if let s = account.fiveHour {
                    HStack(spacing: 3) {
                        Image(systemName: "bolt.fill").font(.system(size: 9))
                        Text("Session \(Int(s.utilization))%")
                    }
                    .font(.caption2).foregroundStyle(.tertiary)
                }
                if let w = account.sevenDay {
                    HStack(spacing: 3) {
                        Image(systemName: "calendar").font(.system(size: 9))
                        Text("Weekly \(Int(w.utilization))%")
                    }
                    .font(.caption2).foregroundStyle(.tertiary)
                }
                if let extra = account.extraUsage, extra.isEnabled {
                    HStack(spacing: 3) {
                        Image(systemName: "dollarsign.circle").font(.system(size: 9))
                        Text("$\(String(format: "%.0f", extra.usedDollars))")
                    }
                    .font(.caption2).foregroundStyle(.tertiary)
                }
            }

            if account.fiveHour == nil && account.sevenDay == nil {
                Text("No data")
                    .font(.caption2).foregroundStyle(.quaternary)
            }
        }
    }
}

// MARK: - Claude CLI Auth

private func claudeAuthLogin() {
    Task.detached {
        let helper = FileManager.default.temporaryDirectory.appendingPathComponent("open-safari.sh")
        try? "#!/bin/bash\nopen -a Safari \"$@\"\n".write(to: helper, atomically: true, encoding: .utf8)
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

// MARK: - Login Status

struct LoginStatusView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            switch appState.loginStep {
            case .waitingForLogin:
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text("Log in via the browser window")
                        .font(.caption).foregroundStyle(.secondary)
                }
            case .extracting:
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text("Extracting account info...")
                        .font(.caption).foregroundStyle(.secondary)
                }
            default:
                EmptyView()
            }

            if let error = appState.loginError {
                Text(error).font(.caption).foregroundStyle(.red)
            }
        }
        .padding(.vertical, 8)
    }
}
