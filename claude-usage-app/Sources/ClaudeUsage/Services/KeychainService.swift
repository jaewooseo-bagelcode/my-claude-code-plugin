import Foundation
import Security

enum KeychainError: Error {
    case saveFailed(OSStatus)
    case loadFailed(OSStatus)
}

enum KeychainService {
    private static let service = "com.sugarscone.claude-usage"
    private static let accountKey = "accounts"

    static func saveAccounts(_ accounts: [Account]) throws {
        let data = try JSONEncoder().encode(accounts)

        // Delete existing
        let deleteQuery: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: accountKey,
        ]
        SecItemDelete(deleteQuery as CFDictionary)

        // Add new
        let addQuery: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: accountKey,
            kSecValueData as String: data,
        ]
        let status = SecItemAdd(addQuery as CFDictionary, nil)
        guard status == errSecSuccess else {
            throw KeychainError.saveFailed(status)
        }
    }

    static func loadAccounts() -> [Account] {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: accountKey,
            kSecReturnData as String: true,
        ]
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        guard status == errSecSuccess, let data = result as? Data else {
            return []
        }
        return (try? JSONDecoder().decode([Account].self, from: data)) ?? []
    }

    // MARK: - Import from Claude Code

    static func readClaudeCodeCredentials() -> ClaudeCodeCredentials? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "Claude Code-credentials",
            kSecReturnData as String: true,
        ]
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        guard status == errSecSuccess, let data = result as? Data else {
            return nil
        }
        return try? JSONDecoder().decode(ClaudeCodeCredentialWrapper.self, from: data).claudeAiOauth
    }
}

// MARK: - Claude Code Credential Models

struct ClaudeCodeCredentialWrapper: Codable {
    let claudeAiOauth: ClaudeCodeCredentials
}

struct ClaudeCodeCredentials: Codable {
    let accessToken: String
    let refreshToken: String
    let expiresAt: Int64 // Unix ms
    let subscriptionType: String?
    let rateLimitTier: String?

    var tokenExpiresAt: Date {
        Date(timeIntervalSince1970: TimeInterval(expiresAt) / 1000)
    }
}
