// Protocol conformance checks for DetourKit: `swift run KitChecks`.
// Wire fixtures live in /fixtures and are shared with the Rust tests.

import CryptoKit
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

// Updater
check(Version("v0.2.10")! > Version("0.2.9")! && Version("1.0")! == Version("1.0.0")! && Version("0.1.0")! < Version("v0.1.1")!,
    "version ordering")
check(Version("") == nil && Version("v1.x") == nil, "rejects malformed versions")
check(UpdateConfig.publicKey.count == 44 && (try? ReleaseVerifier(publicKeyBase64: UpdateConfig.publicKey)) != nil,
    "embedded update key is a valid Ed25519 public key")

let sumsText = "abc123  MacDetour-macos-arm64.zip\nDEF456 *macdetour-macos-arm64.tar.gz\n"
check(ReleaseVerifier.parseSums(sumsText) == ["MacDetour-macos-arm64.zip": "abc123", "macdetour-macos-arm64.tar.gz": "def456"],
    "parses shasum output")

check(try {
    let key = Curve25519.Signing.PrivateKey()
    let other = Curve25519.Signing.PrivateKey()
    let sums = Data(sumsText.utf8)
    let good = try key.signature(for: sums).base64EncodedString()
    let forged = try other.signature(for: sums).base64EncodedString()
    let verifier = try ReleaseVerifier(publicKeyBase64: key.publicKey.rawRepresentation.base64EncodedString())
    let entries = try verifier.verifiedSums(sums: sums, signature: Data(good.utf8))
    let rejectsForged = (try? verifier.verifiedSums(sums: sums, signature: Data(forged.utf8))) == nil
    let rejectsTampered = (try? verifier.verifiedSums(sums: Data((sumsText + "x").utf8), signature: Data(good.utf8))) == nil
    return entries.count == 2 && rejectsForged && rejectsTampered
}(), "accepts only checksums signed by the release key")

check(try {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("kitchecks-\(getpid())")
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: dir) }
    let file = dir.appendingPathComponent("a.zip")
    try Data("hello".utf8).write(to: file)
    let hex = try ReleaseVerifier.sha256Hex(of: file)
    try ReleaseVerifier.check(file, named: "a.zip", in: ["a.zip": hex])
    let mismatch = (try? ReleaseVerifier.check(file, named: "a.zip", in: ["a.zip": String(repeating: "0", count: 64)])) == nil
    let missing = (try? ReleaseVerifier.check(file, named: "a.zip", in: [:])) == nil
    return hex == "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824" && mismatch && missing
}(), "checks downloaded files against their checksums")

print("\(passed) passed, \(failures) failed")
exit(failures == 0 ? 0 : 1)
