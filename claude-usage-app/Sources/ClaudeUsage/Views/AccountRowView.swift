import SwiftUI

struct AccountRowView: View {
    let account: Account
    let onRemove: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            // Header
            HStack {
                Text(account.displayName)
                    .font(.system(.headline, design: .rounded))
                    .lineLimit(1)

                if !account.planType.isEmpty {
                    Text(account.planType.capitalized)
                        .font(.caption2.bold())
                        .padding(.horizontal, 5)
                        .padding(.vertical, 1)
                        .background(.blue.opacity(0.2))
                        .foregroundStyle(.blue)
                        .clipShape(Capsule())
                }

                Spacer()

                if account.error != nil {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.yellow)
                        .help(account.error ?? "")
                }

                Menu {
                    Button("Remove Account", role: .destructive) { onRemove() }
                } label: {
                    Image(systemName: "ellipsis.circle")
                        .foregroundStyle(.secondary)
                }
                .menuIndicator(.hidden)
                .fixedSize()
            }

            // Usage bars
            if let fiveHour = account.fiveHour {
                UsageBarView(
                    icon: "bolt.fill",
                    label: "Session",
                    value: fiveHour.utilization,
                    resetDate: fiveHour.resetDate
                )
            }
            if let sevenDay = account.sevenDay {
                UsageBarView(
                    icon: "calendar",
                    label: "Weekly",
                    value: sevenDay.utilization,
                    resetDate: sevenDay.resetDate
                )
            }

            // Extra usage
            if let extra = account.extraUsage, extra.isEnabled {
                ExtraUsageBarView(extra: extra)
            }

            // No data yet
            if account.fiveHour == nil && account.sevenDay == nil && account.error == nil {
                HStack {
                    ProgressView().controlSize(.small)
                    Text("Loading...")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                }
            }

            // Last updated
            if let lastUpdated = account.lastUpdated {
                Text("Updated \(lastUpdated, style: .relative) ago")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, 4)
    }
}

// MARK: - Usage Bar

struct UsageBarView: View {
    let icon: String
    let label: String
    let value: Double
    let resetDate: Date?

    private var barGradient: LinearGradient {
        if value >= 90 {
            return LinearGradient(colors: [.red, .red.opacity(0.8)], startPoint: .leading, endPoint: .trailing)
        }
        if value >= 75 {
            return LinearGradient(colors: [.orange, .yellow], startPoint: .leading, endPoint: .trailing)
        }
        if value >= 50 {
            return LinearGradient(colors: [.blue, .cyan], startPoint: .leading, endPoint: .trailing)
        }
        return LinearGradient(colors: [.green, .cyan], startPoint: .leading, endPoint: .trailing)
    }

    private var valueColor: Color {
        if value >= 90 { return .red }
        if value >= 75 { return .orange }
        return .primary
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 4) {
                Image(systemName: icon)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                Text(label)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                Text("\(Int(value))%")
                    .font(.system(.caption, design: .rounded).monospacedDigit().bold())
                    .foregroundStyle(valueColor)
            }

            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    RoundedRectangle(cornerRadius: 4)
                        .fill(.primary.opacity(0.08))
                    RoundedRectangle(cornerRadius: 4)
                        .fill(barGradient)
                        .frame(width: max(0, geo.size.width * CGFloat(value / 100)))
                        .shadow(color: barGradient.stops.first?.color.opacity(0.3) ?? .clear, radius: 2, y: 1)
                }
            }
            .frame(height: 8)

            if let resetDate {
                Text("Resets \(resetDate, style: .relative)")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
    }
}

// MARK: - Extra Usage Bar

struct ExtraUsageBarView: View {
    let extra: ExtraUsage

    private var barGradient: LinearGradient {
        if extra.utilization >= 90 {
            return LinearGradient(colors: [.red, .orange], startPoint: .leading, endPoint: .trailing)
        }
        if extra.utilization >= 75 {
            return LinearGradient(colors: [.orange, .yellow], startPoint: .leading, endPoint: .trailing)
        }
        return LinearGradient(colors: [.green, .mint], startPoint: .leading, endPoint: .trailing)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 4) {
                Image(systemName: "dollarsign.circle.fill")
                    .font(.caption2)
                    .foregroundStyle(.green)
                Text("Extra")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                Text("$\(String(format: "%.2f", extra.usedDollars))")
                    .font(.system(.caption, design: .rounded).monospacedDigit().bold())
                +
                Text(" / $\(String(format: "%.0f", extra.limitDollars))")
                    .font(.system(.caption2, design: .rounded).monospacedDigit())
                    .foregroundColor(.secondary)
            }

            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    RoundedRectangle(cornerRadius: 4)
                        .fill(.primary.opacity(0.08))
                    RoundedRectangle(cornerRadius: 4)
                        .fill(barGradient)
                        .frame(width: max(0, geo.size.width * CGFloat(extra.utilization / 100)))
                        .shadow(color: .green.opacity(0.3), radius: 2, y: 1)
                }
            }
            .frame(height: 8)
        }
    }
}

private extension LinearGradient {
    var stops: [Gradient.Stop] {
        []  // Placeholder — shadow color falls back to .clear
    }
}
