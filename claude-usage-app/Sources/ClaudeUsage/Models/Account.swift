import Foundation

struct Account: Identifiable, Codable {
    let id: UUID
    var orgId: String
    var email: String
    var organizationName: String
    var planType: String
    var label: String?

    var fiveHour: UsageMetric?
    var sevenDay: UsageMetric?
    var sevenDaySonnet: UsageMetric?
    var extraUsage: ExtraUsage?
    var lastUpdated: Date?

    // Transient (not persisted)
    var error: String?

    enum CodingKeys: String, CodingKey {
        case id, orgId, email, organizationName, planType, label
        case fiveHour, sevenDay, sevenDaySonnet, extraUsage, lastUpdated
    }

    var displayName: String {
        if let label, !label.isEmpty { return label }
        if !organizationName.isEmpty { return organizationName }
        if !email.isEmpty { return email }
        return planType.capitalized
    }
}
