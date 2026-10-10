// Renders the menu bar panel (light and dark) and a 1600×900 social card from
// sample data, using the app's real SwiftUI views. Needs no daemon and no
// screen-recording permission.
//
//   swift run RenderScreenshots ../docs/images

import AppKit
import DetourKit
import DetourUI
import SwiftUI

let args = CommandLine.arguments
guard args.count == 2 else {
    print("usage: RenderScreenshots <output-dir>")
    exit(2)
}
let outDir = URL(fileURLWithPath: args[1], isDirectory: true)
try FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)

// Sample data from a real match day; the server is a placeholder.
let now = UInt64(Date().timeIntervalSince1970)
let sample = """
{"type":"status","status":{"enabled":true,"phase":"active",
 "tunnel":{"connected":true,"connected_since":\(now - 1800),"reconnects":0,"server":"tunnel@my-server"},
 "routes":{"interface":"utun6","ipv4":270,"ipv6":8,
   "sample":["104.20.18.209","104.21.0.122","172.67.68.4","188.114.96.5","188.114.97.5"]},
 "list":{"fetched_at":\(now - 60),"listed":278,"reachable":0,"unsupported":0,"refreshing":false},
 "primary_interface":"en0","last_error":null}}
"""
guard case let .status(status)? = try? JSONDecoder.daemon.decode(Response.self, from: Data(sample.utf8)) else {
    print("bad sample status")
    exit(1)
}

struct Panel: View {
    let model: AppModel

    var body: some View {
        MenuContent(model: model)
            .background(Color(nsColor: .windowBackgroundColor))
            // The native switch always draws in its inactive grey offscreen, so
            // paint the "on" switch exactly where it sits in the open panel.
            .overlay(alignment: .topTrailing) {
                Capsule()
                    .fill(Color.accentColor)
                    .frame(width: 38, height: 22)
                    .overlay(alignment: .trailing) {
                        Circle().fill(.white).frame(width: 20, height: 20).padding(1)
                            .shadow(color: .black.opacity(0.25), radius: 1, y: 0.5)
                    }
                    .padding(.top, 18)
                    .padding(.trailing, 14)
            }
            .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(.separator, lineWidth: 0.5))
    }
}

/// 800×450 pt (1600×900 px): the panel next to a one-line pitch.
struct SocialCard: View {
    let model: AppModel

    var body: some View {
        HStack(spacing: 48) {
            VStack(alignment: .leading, spacing: 18) {
                Text("MacDetour")
                    .font(.system(size: 44, weight: .bold, design: .rounded))
                Text("Solo las IPs que bloquea LaLiga\nvan por tu servidor. El resto,\npor tu conexión de siempre.")
                    .font(.system(size: 23, weight: .medium))
                    .lineSpacing(3)
                    .foregroundStyle(.white.opacity(0.85))
                    .fixedSize(horizontal: false, vertical: true)
                VStack(alignment: .leading, spacing: 4) {
                    Text("macOS · gratis · open source")
                    Text("github.com/estevecastells/macdetour")
                }
                .font(.system(size: 14, weight: .medium, design: .monospaced))
                .foregroundStyle(.white.opacity(0.6))
                .fixedSize()
            }
            .foregroundStyle(.white)
            .frame(maxWidth: .infinity, alignment: .leading)
            Panel(model: model)
                .shadow(color: .black.opacity(0.45), radius: 24, y: 12)
        }
        .padding(.horizontal, 56)
        .frame(width: 800, height: 450)
        .background(
            LinearGradient(
                colors: [Color(red: 0.07, green: 0.09, blue: 0.16), Color(red: 0.10, green: 0.23, blue: 0.30)],
                startPoint: .topLeading, endPoint: .bottomTrailing))
    }
}

@MainActor
func render<V: View>(_ view: V, dark: Bool, to url: URL) throws {
    let appearance = NSAppearance(named: dark ? .darkAqua : .aqua)!
    let host = NSHostingView(rootView: view)
    host.appearance = appearance
    let size = host.fittingSize
    host.frame = NSRect(origin: .zero, size: size)
    let window = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
    window.appearance = appearance
    window.isOpaque = false
    window.backgroundColor = .clear
    window.contentView = host
    host.layoutSubtreeIfNeeded()
    host.display()

    let scale = 2
    guard
        let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: Int(size.width) * scale, pixelsHigh: Int(size.height) * scale,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
    else { throw CocoaError(.featureUnsupported) }
    rep.size = size
    NSAppearance.current = appearance
    host.cacheDisplay(in: host.bounds, to: rep)
    guard let png = rep.representation(using: .png, properties: [:]) else { throw CocoaError(.fileWriteUnknown) }
    try png.write(to: url)
    print("wrote \(url.path) (\(Int(size.width) * scale)×\(Int(size.height) * scale) px)")
}

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
MainActor.assumeIsolated {
    do {
        let model = AppModel(previewStatus: status)
        try render(Panel(model: model), dark: false, to: outDir.appendingPathComponent("panel-light.png"))
        try render(Panel(model: model), dark: true, to: outDir.appendingPathComponent("panel-dark.png"))
        try render(SocialCard(model: model), dark: true, to: outDir.appendingPathComponent("social-card.png"))
    } catch {
        print("render failed: \(error)")
        exit(1)
    }
}
