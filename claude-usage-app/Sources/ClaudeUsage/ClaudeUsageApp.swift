import SwiftUI

@main
struct ClaudeUsageApp: App {
    @State private var appState = AppState()
    @State private var alertBlink = false
    @State private var blinkTimer: Timer?

    private var isAlert: Bool {
        guard let p = appState.menuBarAccount else { return false }
        return (p.fiveHour?.utilization ?? 0) >= 95
            || (p.sevenDay?.utilization ?? 0) >= 95
    }

    var body: some Scene {
        MenuBarExtra {
            UsagePopoverView()
                .environment(appState)
        } label: {
            Image(nsImage: renderMenuBar())
                .onChange(of: isAlert) { _, alert in
                    if alert {
                        startBlinkTimer()
                    } else {
                        stopBlinkTimer()
                    }
                }
                .onAppear {
                    if isAlert { startBlinkTimer() }
                }
        }
        .menuBarExtraStyle(.window)
    }

    private func startBlinkTimer() {
        guard blinkTimer == nil else { return }
        blinkTimer = Timer.scheduledTimer(withTimeInterval: 0.8, repeats: true) { _ in
            Task { @MainActor in alertBlink.toggle() }
        }
    }

    private func stopBlinkTimer() {
        blinkTimer?.invalidate()
        blinkTimer = nil
        alertBlink = false
    }

    // MARK: - Render colored menu bar image

    private func renderMenuBar() -> NSImage {
        guard let p = appState.menuBarAccount,
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
                MiniBarLabel(value: session, dimmed: alertBlink && session >= 95)

                Text("·")
                    .font(.system(size: 12, weight: .bold, design: .rounded))
                    .foregroundStyle(.white.opacity(0.4))

                // Weekly mini bar + number
                MiniBarLabel(value: weekly, dimmed: alertBlink && weekly >= 95)
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
    var dimmed: Bool = false

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
        .opacity(dimmed ? 0.2 : 1.0)
    }
}
