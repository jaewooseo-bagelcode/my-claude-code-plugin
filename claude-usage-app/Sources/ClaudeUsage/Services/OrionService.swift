import AppKit
import Foundation
import os

private let logger = Logger(subsystem: "com.sugarscone.claude-usage", category: "OrionService")

/// Manages Orion browser profiles and fetches Claude usage data via URLSession + cookies.
final class OrionService: @unchecked Sendable {

    struct OrionProfile {
        let uuid: String   // "default" for main instance
        let name: String
        let appPath: String
    }

    struct OrgInfo {
        let orgId: String
        let planType: String
        let email: String
        let orgName: String
    }

    /// Fixed profile slots: claude-usage-1, claude-usage-2, claude-usage-3
    static let maxSlots = 3

    static let shared = OrionService()

    private let session: URLSession = {
        let config = URLSessionConfiguration.ephemeral
        config.httpShouldSetCookies = false
        config.httpCookieAcceptPolicy = .never
        return URLSession(configuration: config)
    }()

    private let userAgent = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15"

    // MARK: - Profile Discovery (fixed slots: claude-usage-1..3)

    func discoverProfiles() -> [OrionProfile] {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        let profilesDir = "\(home)/Applications/Orion/Orion Profiles"
        var profiles: [OrionProfile] = []

        guard let uuidDirs = try? FileManager.default.contentsOfDirectory(atPath: profilesDir) else {
            return profiles
        }

        for uuidDir in uuidDirs {
            let dirPath = "\(profilesDir)/\(uuidDir)"
            guard let contents = try? FileManager.default.contentsOfDirectory(atPath: dirPath),
                  let appName = contents.first(where: { $0.hasSuffix(".app") })
            else { continue }

            var name = appName
            if name.hasPrefix("Orion - ") { name = String(name.dropFirst(8)) }
            if name.hasSuffix(".app") { name = String(name.dropLast(4)) }

            profiles.append(OrionProfile(uuid: uuidDir, name: name, appPath: "\(dirPath)/\(appName)"))
        }

        // Sort by name so claude-usage-1 < claude-usage-2 < claude-usage-3
        return profiles.sorted { $0.name < $1.name }
    }

    /// Returns only unused slots (profiles not yet linked to any account).
    func availableSlots(usedProfileIds: Set<String>) -> [OrionProfile] {
        discoverProfiles().filter { !usedProfileIds.contains($0.uuid) }
    }

    // MARK: - Cookie Reading

    func cookiePath(for profileUUID: String) -> String? {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        let fm = FileManager.default

        if profileUUID == "default" {
            // Main Orion instance cookies
            let paths = [
                "\(home)/Library/HTTPStorages/com.kagi.kagimacOS.binarycookies",
                "\(home)/Library/Application Support/Orion/Defaults/cookies",
            ]
            return paths.first { fm.fileExists(atPath: $0) }
        } else {
            // Profile cookies
            let paths = [
                "\(home)/Library/Application Support/Orion/\(profileUUID)/cookies",
                "\(home)/Library/HTTPStorages/com.kagi.kagimacOS.\(profileUUID).binarycookies",
            ]
            return paths.first { fm.fileExists(atPath: $0) }
        }
    }

    func readClaudeCookies(profileUUID: String) throws -> [BinaryCookie] {
        guard let path = cookiePath(for: profileUUID) else {
            throw OrionError.cookieFileNotFound
        }
        let all = try BinaryCookieParser.parse(fileAt: path)
        return all.filter { $0.domain.contains("claude.ai") }
    }

    func sessionKey(profileUUID: String) throws -> String? {
        let cookies = try readClaudeCookies(profileUUID: profileUUID)
        return cookies.first(where: { $0.name == "sessionKey" })?.value
    }

    // MARK: - API Calls via URLSession

    private func makeRequest(url: String, cookies: [BinaryCookie]) -> URLRequest {
        var request = URLRequest(url: URL(string: url)!)
        request.setValue(userAgent, forHTTPHeaderField: "User-Agent")
        request.setValue("https://claude.ai", forHTTPHeaderField: "Origin")
        request.setValue("https://claude.ai/settings/usage", forHTTPHeaderField: "Referer")
        request.setValue("application/json", forHTTPHeaderField: "Accept")

        // Build cookie header from all claude.ai cookies
        let cookieStr = cookies.map { "\($0.name)=\($0.value)" }.joined(separator: "; ")
        request.setValue(cookieStr, forHTTPHeaderField: "Cookie")

        return request
    }

    func fetchUsage(profileUUID: String, orgId: String) async throws -> UsageResponse {
        let cookies = try readClaudeCookies(profileUUID: profileUUID)
        guard !cookies.isEmpty else { throw OrionError.notLoggedIn }

        let request = makeRequest(
            url: "https://claude.ai/api/organizations/\(orgId)/usage",
            cookies: cookies
        )

        let (data, response) = try await session.data(for: request)

        guard let httpResponse = response as? HTTPURLResponse else {
            throw OrionError.invalidResponse
        }

        if httpResponse.statusCode == 401 || httpResponse.statusCode == 403 {
            throw OrionError.sessionExpired
        }

        guard httpResponse.statusCode == 200 else {
            throw OrionError.apiFailed("HTTP \(httpResponse.statusCode)")
        }

        return try JSONDecoder().decode(UsageResponse.self, from: data)
    }

    func extractOrganizations(profileUUID: String) async throws -> [OrgInfo] {
        let cookies = try readClaudeCookies(profileUUID: profileUUID)
        guard !cookies.isEmpty else { throw OrionError.notLoggedIn }

        // Fetch organizations
        let orgsRequest = makeRequest(url: "https://claude.ai/api/organizations", cookies: cookies)
        let (orgsData, orgsResponse) = try await session.data(for: orgsRequest)

        guard let httpResponse = orgsResponse as? HTTPURLResponse, httpResponse.statusCode == 200 else {
            throw OrionError.apiFailed("Failed to fetch organizations")
        }

        guard let orgsArray = try JSONSerialization.jsonObject(with: orgsData) as? [[String: Any]] else {
            throw OrionError.invalidResponse
        }

        // Fetch email
        let emailRequest = makeRequest(url: "https://claude.ai/api/auth/current_account", cookies: cookies)
        var email = ""
        if let (emailData, _) = try? await session.data(for: emailRequest),
           let account = try? JSONSerialization.jsonObject(with: emailData) as? [String: Any]
        {
            email = account["email_address"] as? String
                ?? account["email"] as? String
                ?? account["primary_email"] as? String
                ?? ""
        }

        // Fallback: extract email from org name
        if email.isEmpty {
            for org in orgsArray {
                let name = org["name"] as? String ?? ""
                if let range = name.range(of: #"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}"#, options: .regularExpression) {
                    email = String(name[range])
                    break
                }
            }
        }

        // Build org infos (filter chat-capable orgs only)
        var results: [OrgInfo] = []
        for org in orgsArray {
            guard let orgId = org["uuid"] as? String, !orgId.isEmpty else { continue }
            let capabilities = org["capabilities"] as? [String] ?? []
            guard capabilities.contains("chat") else { continue }

            let name = org["name"] as? String ?? ""
            var plan = ""
            if capabilities.contains("raven_enterprise") { plan = "enterprise" }
            else if capabilities.contains("claude_max") { plan = "claude_max" }
            else if capabilities.contains("raven") { plan = "pro" }
            else { plan = "free" }

            results.append(OrgInfo(orgId: orgId, planType: plan, email: email, orgName: name))
        }

        return results
    }

    // MARK: - Browser Control

    func openProfile(_ profileUUID: String, url: String? = nil) {
        let profiles = discoverProfiles()
        guard let profile = profiles.first(where: { $0.uuid == profileUUID }) else {
            logger.error("Profile not found: \(profileUUID)")
            return
        }

        let appURL = URL(fileURLWithPath: profile.appPath)
        let config = NSWorkspace.OpenConfiguration()

        if let urlStr = url, let targetURL = URL(string: urlStr) {
            NSWorkspace.shared.open([targetURL], withApplicationAt: appURL, configuration: config)
        } else {
            NSWorkspace.shared.openApplication(at: appURL, configuration: config)
        }
    }

    /// Wait for sessionKey cookie to appear in profile's cookie file.
    func waitForLogin(profileUUID: String, timeout: TimeInterval = 300) async throws {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            try Task.checkCancellation()
            if let key = try? sessionKey(profileUUID: profileUUID), !key.isEmpty {
                return
            }
            try await Task.sleep(for: .seconds(2))
        }
        throw OrionError.timeout
    }

    // MARK: - Auto-create profiles (claude-usage-1..3)

    /// Ensures claude-usage-1..3 profiles exist. Creates missing ones.
    /// Must be called when Orion is NOT running (profiles plist gets overwritten otherwise).
    func ensureProfilesExist() {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        let profilesDir = "\(home)/Applications/Orion/Orion Profiles"
        let fm = FileManager.default

        guard fm.fileExists(atPath: "/Applications/Orion.app") else { return }

        // Read existing profiles plist
        let plistPath = "\(home)/Library/Application Support/Orion/profiles"
        var existingUUIDs: [String: String] = [:]  // name -> uuid
        if let data = fm.contents(atPath: plistPath),
           let plist = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any],
           let profiles = plist["profiles"] as? [[String: Any]]
        {
            for p in profiles {
                if let name = p["name"] as? String, let id = p["identifier"] as? String {
                    existingUUIDs[name] = id
                }
            }
        }

        let colors = [1, 3, 5]  // blue, orange, purple
        var profileEntries: [[String: Any]] = []

        for i in 1...Self.maxSlots {
            let name = "claude-usage-\(i)"

            if let uuid = existingUUIDs[name] {
                // Profile already registered
                profileEntries.append(["name": name, "color": colors[i-1], "identifier": uuid])
                continue
            }

            // Create new profile
            let uuid = UUID().uuidString
            let appDir = "\(profilesDir)/\(uuid)/Orion - \(name).app"

            try? fm.createDirectory(atPath: "\(appDir)/Contents/MacOS", withIntermediateDirectories: true)
            try? fm.createDirectory(atPath: "\(appDir)/Contents/Resources", withIntermediateDirectories: true)

            // Copy icon
            if let iconData = fm.contents(atPath: "/Applications/Orion.app/Contents/Resources/AppIcon.icns") {
                fm.createFile(atPath: "\(appDir)/Contents/Resources/AppIcon.icns", contents: iconData)
            }

            // Launcher script
            let script = "#!/bin/bash\n\nexec arch -arm64 \"/Applications/Orion.app/Contents/MacOS/Orion\" -P \(uuid)\n"
            try? script.write(toFile: "\(appDir)/Contents/MacOS/Orion", atomically: true, encoding: .utf8)
            try? fm.setAttributes([.posixPermissions: 0o755], ofItemAtPath: "\(appDir)/Contents/MacOS/Orion")

            // Info.plist
            let plist = """
            <?xml version="1.0" encoding="UTF-8"?>
            <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
            <plist version="1.0"><dict>
            <key>CFBundleExecutable</key><string>Orion</string>
            <key>CFBundleIconFile</key><string>AppIcon</string>
            <key>CFBundleIdentifier</key><string>com.kagi.kagimacOS.\(uuid)</string>
            <key>CFBundleName</key><string>\(name)</string>
            <key>CFBundleShortVersionString</key><string>1.0.4</string>
            <key>CFBundleVersion</key><string>143.1</string>
            <key>CFBundleDocumentTypes</key><array><dict>
            <key>CFBundleTypeName</key><string>HTML Document</string>
            <key>CFBundleTypeRole</key><string>Viewer</string>
            <key>LSHandlerRank</key><string>Default</string>
            <key>LSItemContentTypes</key><array><string>public.html</string><string>public.xhtml</string></array>
            </dict></array>
            <key>CFBundleURLTypes</key><array><dict>
            <key>CFBundleTypeRole</key><string>Viewer</string>
            <key>CFBundleURLSchemes</key><array><string>https</string><string>http</string><string>file</string></array>
            </dict></array>
            <key>LSArchitecturePriority</key><array><string>arm64</string><string>x86_64</string></array>
            <key>LSMinimumSystemVersion</key><string>10.14</string>
            <key>SUBundleName</key><string>Orion</string>
            </dict></plist>
            """
            try? plist.write(toFile: "\(appDir)/Contents/Info.plist", atomically: true, encoding: .utf8)

            // Data directory
            try? fm.createDirectory(atPath: "\(home)/Library/Application Support/Orion/\(uuid)", withIntermediateDirectories: true)

            profileEntries.append(["name": name, "color": colors[i-1], "identifier": uuid])
            logger.info("Created Orion profile: \(name) (\(uuid))")
        }

        // Write profiles plist
        let fullPlist: [String: Any] = [
            "defaults": ["color": 7, "identifier": "Defaults", "name": "Primary"],
            "profiles": profileEntries,
        ]
        if let data = try? PropertyListSerialization.data(fromPropertyList: fullPlist, format: .binary, options: 0) {
            fm.createFile(atPath: plistPath, contents: data)
        }
    }
}

// MARK: - Errors

enum OrionError: LocalizedError {
    case cookieFileNotFound
    case notLoggedIn
    case sessionExpired
    case invalidResponse
    case apiFailed(String)
    case timeout
    case profileNotFound

    var errorDescription: String? {
        switch self {
        case .cookieFileNotFound: return "Cookie file not found — is Orion installed?"
        case .notLoggedIn: return "Not logged in — open browser to log in"
        case .sessionExpired: return "Session expired — re-login needed"
        case .invalidResponse: return "Invalid API response"
        case .apiFailed(let msg): return msg
        case .timeout: return "Timed out waiting for login"
        case .profileNotFound: return "Orion profile not found"
        }
    }
}
