// Protocol conformance checks for DetourKit: `swift run KitChecks`.
// Wire fixtures live in /fixtures and are shared with the Rust tests.

import DetourKit
import Foundation

var failures = 0
var passed = 0

func check(_ condition: @autoclosure () throws -> Bool, _ name: String, line: UInt = #line) {
    do {
        if try condition() { passed += 1 } else { failures += 1; print("FAIL \(name) (line \(line))") }
    } catch {
        failures += 1
        print("FAIL \(name): threw \(error)")
    }
}

let repoRoot = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
func fixture(_ name: String) -> Data {
    try! Data(contentsOf: repoRoot.appendingPathComponent("fixtures/\(name)"))
}

check(try {
    guard case let .status(s) = try JSONDecoder.daemon.decode(Response.self, from: fixture("status.json")) else { return false }
    return s.enabled && s.phase == .active && s.tunnel.connected && s.tunnel.connectedSince == 1_760_112_000
        && s.routes.interface == "utun7" && s.routes.total == 2 && s.routes.sample == ["188.114.96.5", "188.114.97.5"]
        && s.list.listed == 263 && s.list.fetchedAt == 1_760_112_030 && s.primaryInterface == "en0" && s.lastError == nil
}(), "decodes an active status")

check(try {
    guard case let .status(s) = try JSONDecoder.daemon.decode(Response.self, from: fixture("status-off.json")) else { return false }
    return !s.enabled && s.phase == .off && s.routes.interface == nil && s.list.fetchedAt == nil
        && s.lastError?.contains("publickey") == true
}(), "decodes an off status with an error")

check(try {
    let e = JSONEncoder()
    e.outputFormatting = .sortedKeys
    let mine = try [Request.ping, .status, .setEnabled(true), .refresh].map { String(decoding: try e.encode($0), as: UTF8.self) }
    let theirs = try (JSONSerialization.jsonObject(with: fixture("requests.json")) as! [[String: Any]]).map {
        String(decoding: try JSONSerialization.data(withJSONObject: $0, options: .sortedKeys), as: UTF8.self)
    }
    return mine == theirs
}(), "requests encode exactly like the fixtures")

check(try {
    guard case let .error(m) = try JSONDecoder.daemon.decode(Response.self, from: Data(#"{"type":"error","message":"permission denied for uid 502"}"#.utf8)) else { return false }
    return m.contains("502")
}(), "decodes errors")

check(Describe.ago(nil) == "nunca" && Describe.ago(100, now: Date(timeIntervalSince1970: 130)) == "hace 30 s"
    && Describe.ago(100, now: Date(timeIntervalSince1970: 400)) == "hace 5 min", "relative times")

print("\(passed) passed, \(failures) failed")
exit(failures == 0 ? 0 : 1)
