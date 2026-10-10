// Mirrors the daemon's JSON protocol (crates/detour-core/src/protocol.rs).
// Keys arrive in snake_case; decode with `JSONDecoder.daemon`.

import Foundation

public enum Phase: String, Decodable, Sendable {
    case off, connecting, active, reconnecting
}

public struct TunnelStatus: Decodable, Equatable, Sendable {
    public var connected: Bool
    public var connectedSince: UInt64?
    public var reconnects: UInt32
    public var server: String
}

public struct RouteStatus: Decodable, Equatable, Sendable {
    public var interface: String?
    public var ipv4: Int
    public var ipv6: Int
    public var sample: [String]

    public var total: Int { ipv4 + ipv6 }
}

public struct ListStatus: Decodable, Equatable, Sendable {
    public var fetchedAt: UInt64?
    public var listed: Int
    public var reachable: Int
    public var unsupported: Int
    public var refreshing: Bool
}

public struct Status: Decodable, Equatable, Sendable {
    public var enabled: Bool
    public var phase: Phase
    public var tunnel: TunnelStatus
    public var routes: RouteStatus
    public var list: ListStatus
    public var primaryInterface: String?
    public var lastError: String?
}

public enum Request: Encodable, Sendable {
    case ping
    case status
    case setEnabled(Bool)
    case refresh

    private enum CodingKeys: String, CodingKey { case type, enabled }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .ping: try c.encode("ping", forKey: .type)
        case .status: try c.encode("status", forKey: .type)
        case let .setEnabled(on):
            try c.encode("set_enabled", forKey: .type)
            try c.encode(on, forKey: .enabled)
        case .refresh: try c.encode("refresh", forKey: .type)
        }
    }
}

public enum Response: Decodable, Sendable {
    case pong(version: String, protocolVersion: Int)
    case status(Status)
    case error(String)
    case other(String)

    private enum CodingKeys: String, CodingKey { case type, version, `protocol`, status, message }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(String.self, forKey: .type) {
        case "pong":
            self = .pong(
                version: try c.decode(String.self, forKey: .version),
                protocolVersion: try c.decode(Int.self, forKey: .protocol))
        case "status": self = .status(try c.decode(Status.self, forKey: .status))
        case "error": self = .error(try c.decode(String.self, forKey: .message))
        case let other: self = .other(other)
        }
    }
}

extension JSONDecoder {
    public static var daemon: JSONDecoder {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        return d
    }
}

/// Short Spanish descriptions for the menu.
public enum Describe {
    public static func phase(_ s: Status?) -> String {
        guard let s else { return "Sin servicio" }
        switch s.phase {
        case .off: return "Desactivado"
        case .connecting: return "Conectando…"
        case .active: return "Activo"
        case .reconnecting: return "Reconectando…"
        }
    }

    public static func routes(_ r: RouteStatus) -> String {
        switch r.total {
        case 0: return "Ninguna IP desviada"
        case 1: return "1 IP desviada"
        default: return "\(r.total) IPs desviadas"
        }
    }

    public static func ago(_ unix: UInt64?, now: Date = Date()) -> String {
        guard let unix else { return "nunca" }
        let secs = max(0, Int(now.timeIntervalSince1970) - Int(unix))
        if secs < 60 { return "hace \(secs) s" }
        if secs < 3600 { return "hace \(secs / 60) min" }
        return "hace \(secs / 3600) h"
    }
}
