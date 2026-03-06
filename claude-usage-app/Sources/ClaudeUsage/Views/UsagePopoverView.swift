import SwiftUI

struct UsagePopoverView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Claude Usage")
                    .font(.headline)
                Spacer()
                Button {
                    appState.refreshAll()
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .buttonStyle(.borderless)
                .help("Refresh now")
            }
            .padding(.bottom, 8)

            Divider()

            // Account list
            if appState.accounts.isEmpty && appState.loginStep == .idle {
                VStack(spacing: 8) {
                    Image(systemName: "person.crop.circle.badge.plus")
                        .font(.largeTitle)
                        .foregroundStyle(.secondary)
                    Text("No accounts")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 20)
            } else if !appState.accounts.isEmpty {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 8) {
                        ForEach(appState.accounts) { account in
                            AccountRowView(account: account) {
                                appState.removeAccount(account)
                            }
                            if account.id != appState.accounts.last?.id {
                                Divider()
                            }
                        }
                    }
                }
                .frame(maxHeight: 400)
                .padding(.vertical, 8)
            }

            // Login status
            if appState.loginStep != .idle {
                Divider()
                LoginStatusView()
                    .environment(appState)
            }

            Divider()

            // Footer
            HStack {
                if appState.loginStep == .idle {
                    Button {
                        appState.beginAddAccount()
                    } label: {
                        Label("Add Account", systemImage: "plus.circle")
                    }
                    .buttonStyle(.borderless)
                } else {
                    Button {
                        appState.cancelAddAccount()
                    } label: {
                        Label("Cancel", systemImage: "xmark.circle")
                    }
                    .buttonStyle(.borderless)
                    .foregroundStyle(.secondary)
                }

                Button {
                    claudeAuthLogin()
                } label: {
                    Label("CLI Login", systemImage: "terminal")
                }
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .help("Run 'claude auth login' via Safari")

                Spacer()

                Button("Quit") {
                    NSApplication.shared.terminate(nil)
                }
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
            }
            .padding(.top, 8)
        }
        .padding()
        .frame(width: 320)
    }
}

// MARK: - Claude CLI Auth

/// Runs `claude auth login` with Safari as the browser
private func claudeAuthLogin() {
    Task.detached {
        // Write a helper script that opens URLs in Safari
        let helper = FileManager.default.temporaryDirectory.appendingPathComponent("open-safari.sh")
        try? "#!/bin/bash\nopen -a Safari \"$@\"\n".write(to: helper, atomically: true, encoding: .utf8)
        try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: helper.path)

        let process = Process()
        // Try common paths for claude binary
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
