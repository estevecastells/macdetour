import DetourKit
import Foundation
import Observation
import ServiceManagement

public enum Connection: Equatable {
    case connecting
    case connected
    /// Daemon not installed or not running.
    case missing
    case failed(String)
}

@MainActor
@Observable
public final class AppModel {
    public private(set) var status: Status?
    public private(set) var connection: Connection = .connecting
    private(set) var actionError: String?
    private(set) var busy = false
    private(set) var openAtLogin = false

    private let client = DaemonClient()
    private var pollTask: Task<Void, Never>?

    /// A status call is a single local socket round-trip.
    static let pollInterval: Duration = .seconds(2)

    public init() {
        configureLoginItemOnFirstLaunch()
        start()
    }

    public func start() {
        pollTask?.cancel()
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                await self?.refresh()
                try? await Task.sleep(for: AppModel.pollInterval)
            }
        }
    }

    func refresh() async {
        do {
            status = try await client.status()
            connection = .connected
        } catch DaemonError.notRunning {
            status = nil
            connection = .missing
        } catch {
            connection = .failed(error.localizedDescription)
        }
    }

    public var isOn: Bool { status?.enabled ?? false }

    func setEnabled(_ on: Bool) {
        guard !busy else { return }
        busy = true
        Task {
            defer { busy = false }
            do {
                status = try await client.setEnabled(on)
                actionError = nil
            } catch {
                actionError = error.localizedDescription
            }
        }
    }

    func refreshList() {
        Task {
            do {
                status = try await client.refreshList()
                actionError = nil
            } catch {
                actionError = error.localizedDescription
            }
        }
    }

    // MARK: Open at login

    private static let loginItemConfiguredKey = "loginItemConfigured"

    private func configureLoginItemOnFirstLaunch() {
        let defaults = UserDefaults.standard
        if !defaults.bool(forKey: Self.loginItemConfiguredKey) {
            defaults.set(true, forKey: Self.loginItemConfiguredKey)
            setOpenAtLogin(true)
        }
        let state = SMAppService.mainApp.status
        openAtLogin = state == .enabled || state == .requiresApproval
    }

    func setOpenAtLogin(_ enabled: Bool) {
        do {
            if enabled {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
        } catch {
            actionError = "No se pudo cambiar el inicio automático: \(error.localizedDescription)"
        }
        let state = SMAppService.mainApp.status
        openAtLogin = state == .enabled || state == .requiresApproval
    }
}
