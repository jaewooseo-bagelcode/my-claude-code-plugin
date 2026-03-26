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
                    .help("Add Account (via Orion profile)")
                } else {
                    Button { appState.cancelAddAccount() } label: {
                        Image(systemName: "xmark.circle")
                    }
                    .buttonStyle(.borderless)
                    .help("Cancel")
                }

                Button { appState.refreshAll() } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .buttonStyle(.borderless)
                .help("Refresh All")

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

            // Login flow
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
                    Text("Create Orion profiles, then add accounts")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 16)
            }

            // All accounts grouped by email, ordered by Orion profile slot name
            let profileMap = Dictionary(
                uniqueKeysWithValues: appState.orion.discoverProfiles().map { ($0.uuid, $0.name) }
            )
            let groups = Dictionary(grouping: appState.accounts, by: \.email)
            let orderedEmails = groups.keys.sorted { a, b in
                let aName = profileMap[groups[a]?.first?.orionProfileId ?? ""] ?? "z"
                let bName = profileMap[groups[b]?.first?.orionProfileId ?? ""] ?? "z"
                return aName < bName
            }
            ForEach(Array(orderedEmails.enumerated()), id: \.element) { idx, email in
                if idx > 0 { Divider() }
                if let group = groups[email] {
                    AccountGroupView(email: email, accounts: group) {
                        appState.removeAccountGroup(email: email)
                    }
                    .padding(.vertical, 6)
                }
            }
        }
        .padding()
        .frame(width: 320)
    }
}

// MARK: - Account Group (email -> multiple orgs)

struct AccountGroupView: View {
    @Environment(AppState.self) private var appState
    let email: String
    let accounts: [Account]
    let onRemoveGroup: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            // Group header
            HStack {
                Text(email.isEmpty ? "Unknown" : email)
                    .font(.system(.subheadline, design: .rounded))
                    .lineLimit(1)

                Spacer()

                Menu {
                    if let account = accounts.first {
                        Button {
                            appState.showBrowser(for: account)
                        } label: {
                            Label("Open Browser", systemImage: "globe")
                        }

                        Button {
                            appState.claudeAuthLogin(for: account)
                        } label: {
                            Label("CLI Login", systemImage: "terminal")
                        }

                        Divider()
                    }
                    Button("Remove All", role: .destructive) { onRemoveGroup() }
                } label: {
                    Image(systemName: "ellipsis.circle")
                        .foregroundStyle(.secondary)
                }
                .menuIndicator(.hidden)
                .fixedSize()
            }

            // Org rows — all with full detail
            ForEach(accounts) { account in
                OrgRow(account: account)
            }
        }
    }
}

// MARK: - Org Row (full bars for all accounts)

struct OrgRow: View {
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
                             value: fiveHour.utilization ?? 0, resetDate: fiveHour.resetDate)
                    .saturation(isMenuBarSource ? 1.0 : 0.3)
            }
            if let sevenDay = account.sevenDay {
                UsageBarView(icon: "calendar", label: "Weekly",
                             value: sevenDay.utilization ?? 0, resetDate: sevenDay.resetDate)
                    .saturation(isMenuBarSource ? 1.0 : 0.3)
            }
            if let extra = account.extraUsage, extra.isEnabled == true {
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

// MARK: - Login Status

struct LoginStatusView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            switch appState.loginStep {
            case .pickProfile:
                Text("Select Orion profile:")
                    .font(.caption).foregroundStyle(.secondary)

                ForEach(appState.availableProfiles, id: \.uuid) { profile in
                    Button {
                        appState.selectProfile(profile)
                    } label: {
                        HStack(spacing: 6) {
                            Image(systemName: "globe")
                                .font(.caption)
                            Text(profile.name)
                                .font(.caption)
                        }
                    }
                    .buttonStyle(.borderless)
                }

            case .waitingForLogin:
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text("Log in via Orion browser...")
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
