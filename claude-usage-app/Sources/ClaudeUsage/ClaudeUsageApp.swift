import SwiftUI

@main
struct ClaudeUsageApp: App {
    @State private var appState = AppState()

    var body: some Scene {
        MenuBarExtra {
            UsagePopoverView()
                .environment(appState)
        } label: {
            Image(nsImage: renderMenuBar())
        }
        .menuBarExtraStyle(.window)
    }

    // MARK: - Render colored menu bar image

    private func renderMenuBar() -> NSImage {
        guard let p = appState.activeAccounts.first,
              let session = p.fiveHour?.utilization,
              let weekly = p.sevenDay?.utilization
        else {
            return renderView {
                HStack(spacing: 3) {
                    Image(systemName: "gauge.open.with.lines.needle.33percent")
                        .foregroundStyle(.white.opacity(0.6))
                    Text("--·--")
                        .font(.system(size: 11, weight: .medium, design: .rounded))
                        .monospacedDigit()
                        .foregroundStyle(.white.opacity(0.6))
                }
            }
        }

        return renderView {
            HStack(spacing: 5) {
                // Session mini bar + number
                MiniBarLabel(value: session)

                Text("·")
                    .font(.system(size: 12, weight: .bold, design: .rounded))
                    .foregroundStyle(.white.opacity(0.4))

                // Weekly mini bar + number
                MiniBarLabel(value: weekly)
            }
        }
    }

    private func renderView<V: View>(@ViewBuilder content: () -> V) -> NSImage {
        let view = content()
            .padding(.horizontal, 2)
            .padding(.vertical, 1)

        let renderer = ImageRenderer(content: view)
        renderer.scale = 2.0

        guard let cgImage = renderer.cgImage else {
            let fallback = NSImage(size: NSSize(width: 44, height: 18))
            return fallback
        }

        let image = NSImage(
            cgImage: cgImage,
            size: NSSize(width: cgImage.width / 2, height: cgImage.height / 2)
        )
        image.isTemplate = false
        return image
    }
}

// MARK: - Mini Bar for menu bar

struct MiniBarLabel: View {
    let value: Double

    private var barColor: Color {
        if value >= 90 { return .red }
        if value >= 75 { return .orange }
        if value >= 50 { return .yellow }
        return .green
    }

    private var textColor: Color {
        if value >= 90 { return .red }
        if value >= 75 { return .orange }
        return .white
    }

    var body: some View {
        HStack(spacing: 3) {
            // Mini vertical bar
            ZStack(alignment: .bottom) {
                RoundedRectangle(cornerRadius: 1.5)
                    .fill(.white.opacity(0.15))
                    .frame(width: 4, height: 14)
                RoundedRectangle(cornerRadius: 1.5)
                    .fill(barColor)
                    .frame(width: 4, height: max(1, 14 * value / 100))
            }

            Text("\(Int(value))")
                .font(.system(size: 12, weight: .bold, design: .rounded))
                .monospacedDigit()
                .foregroundStyle(textColor)
        }
    }
}
