import Foundation

struct UsageResponse: Codable {
    let fiveHour: UsageMetric?
    let sevenDay: UsageMetric?
    let sevenDaySonnet: UsageMetric?
    let sevenDayOpus: UsageMetric?
    let sevenDayCowork: UsageMetric?
    let sevenDayOauthApps: UsageMetric?
    let extraUsage: ExtraUsage?

    enum CodingKeys: String, CodingKey {
        case fiveHour = "five_hour"
        case sevenDay = "seven_day"
        case sevenDaySonnet = "seven_day_sonnet"
        case sevenDayOpus = "seven_day_opus"
        case sevenDayCowork = "seven_day_cowork"
        case sevenDayOauthApps = "seven_day_oauth_apps"
        case extraUsage = "extra_usage"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        fiveHour = try c.decodeIfPresent(UsageMetric.self, forKey: .fiveHour)
        sevenDay = try c.decodeIfPresent(UsageMetric.self, forKey: .sevenDay)
        sevenDaySonnet = try c.decodeIfPresent(UsageMetric.self, forKey: .sevenDaySonnet)
        sevenDayOpus = try c.decodeIfPresent(UsageMetric.self, forKey: .sevenDayOpus)
        sevenDayCowork = try c.decodeIfPresent(UsageMetric.self, forKey: .sevenDayCowork)
        sevenDayOauthApps = try c.decodeIfPresent(UsageMetric.self, forKey: .sevenDayOauthApps)
        extraUsage = try c.decodeIfPresent(ExtraUsage.self, forKey: .extraUsage)
    }
}

struct UsageMetric: Codable {
    let utilization: Double?
    let resetsAt: String?

    enum CodingKeys: String, CodingKey {
        case utilization
        case resetsAt = "resets_at"
    }

    var resetDate: Date? {
        guard let resetsAt else { return nil }
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.date(from: resetsAt)
    }
}

struct ExtraUsage: Codable {
    let isEnabled: Bool?
    let monthlyLimit: Int?
    let usedCredits: Double?
    let utilization: Double?

    enum CodingKeys: String, CodingKey {
        case isEnabled = "is_enabled"
        case monthlyLimit = "monthly_limit"
        case usedCredits = "used_credits"
        case utilization
    }

    /// Used amount in dollars (API returns cents)
    var usedDollars: Double { (usedCredits ?? 0) / 100.0 }
    /// Monthly limit in dollars
    var limitDollars: Double { Double(monthlyLimit ?? 0) / 100.0 }
}
