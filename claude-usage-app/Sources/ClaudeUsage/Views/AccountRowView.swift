import SwiftUI

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
        let util = extra.utilization ?? 0
        if util >= 90 {
            return LinearGradient(colors: [.red, .orange], startPoint: .leading, endPoint: .trailing)
        }
        if util >= 75 {
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
                        .frame(width: max(0, geo.size.width * CGFloat((extra.utilization ?? 0) / 100)))
                }
            }
            .frame(height: 8)
        }
    }
}
