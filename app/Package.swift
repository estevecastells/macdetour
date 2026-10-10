// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "MacDetour",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "MacDetour", targets: ["MacDetour"]),
    ],
    targets: [
        // Protocol models and the daemon socket client. No UI; fully testable.
        .target(name: "DetourKit"),
        // SwiftUI views and the app model.
        .target(name: "DetourUI", dependencies: ["DetourKit"]),
        // The menu bar app.
        .executableTarget(name: "MacDetour", dependencies: ["DetourUI"]),
        // Renders the panel and the social card to PNG: `swift run RenderScreenshots <out-dir>`.
        .executableTarget(name: "RenderScreenshots", dependencies: ["DetourUI", "DetourKit"]),
        // Self-checking test runner (XCTest isn't available with Command Line Tools alone).
        .executableTarget(name: "KitChecks", dependencies: ["DetourKit"], path: "Sources/KitChecks"),
    ]
)
