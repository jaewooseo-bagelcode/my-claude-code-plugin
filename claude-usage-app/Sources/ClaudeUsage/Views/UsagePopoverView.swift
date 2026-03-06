import SwiftUI

struct UsagePopoverView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Header: title + all action buttons
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

            // Active account (full detail)
            if let active = appState.activeAccount {
                AccountRowView(account: active, isActive: true) {
                    appState.removeAccount(active)
                }
                .padding(.vertical, 8)
            } else if appState.accounts.isEmpty && appState.loginStep == .idle {
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

            // Inactive accounts (compact)
            ForEach(appState.inactiveAccounts) { account in
                Divider()
                AccountRowView(account: account, isActive: false) {
                    appState.removeAccount(account)
                }
                .padding(.vertical, 6)
            }
        }
        .padding()
        .frame(width: 320)
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
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            case .extracting:
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text("Extracting account info...")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            default:
                EmptyView()
            }

            if let error = appState.loginError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            }
        }
        .padding(.vertical, 8)
    }
}
