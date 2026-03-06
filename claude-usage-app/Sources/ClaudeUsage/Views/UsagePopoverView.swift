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
