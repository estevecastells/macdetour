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
    private var panelOpen = false

    /// While the panel is open the numbers should feel live.
    static let openPollInterval: Duration = .seconds(2)
    /// While closed only the menu bar icon is visible, and it changes rarely.
    static let closedPollInterval: Duration = .seconds(15)

    public init() {
        configureLoginItemOnFirstLaunch()
        start()
    }

    public func start() {
        pollTask?.cancel()
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                await self.refresh()
                let interval = self.panelOpen ? AppModel.openPollInterval : AppModel.closedPollInterval
                try? await Task.sleep(for: interval)
            }
        }
    }

    /// Refresh at once when the panel opens, then keep the faster pace.
    func panelDidOpen() {
        panelOpen = true
        start()
    }

    func panelDidClose() {
        panelOpen = false
    }

    func refresh() async {
        do {
            apply(try await client.status())
            setConnection(.connected)
        } catch DaemonError.notRunning {
            apply(nil)
            setConnection(.missing)
        } catch {
            setConnection(.failed(error.localizedDescription))
        }
    }

    /// Observation invalidates views on every assignment, even of an equal
    /// value, so only publish real changes. Most polls change nothing.
    private func apply(_ new: Status?) {
        if new != status { status = new }
    }

    private func setConnection(_ new: Connection) {
        if new != connection { connection = new }
    }

    public var isOn: Bool { status?.enabled ?? false }

    func setEnabled(_ on: Bool) {
        guard !busy else { return }
        busy = true
        Task {
            defer { busy = false }
            do {
                apply(try await client.setEnabled(on))
                actionError = nil
            } catch {
                actionError = error.localizedDescription
            }
        }
    }

    func refreshList() {
        Task {
            do {
                apply(try await client.refreshList())
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
