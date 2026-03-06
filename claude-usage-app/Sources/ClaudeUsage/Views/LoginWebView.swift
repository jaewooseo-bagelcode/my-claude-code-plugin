import SwiftUI
import WebKit

struct LoginWebView: NSViewRepresentable {
    let webView: WKWebView

    func makeNSView(context: Context) -> WKWebView {
        // Ensure WKWebView can accept focus and keyboard input
        DispatchQueue.main.async {
            webView.window?.makeFirstResponder(webView)
        }
        return webView
    }

    func updateNSView(_ nsView: WKWebView, context: Context) {
        DispatchQueue.main.async {
            nsView.window?.makeFirstResponder(nsView)
        }
    }
}
