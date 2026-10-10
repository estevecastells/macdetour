import AppKit
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
    /// The service bundled with this app is newer than the running one (after an app update).
    private(set) var serviceUpdateAvailable = false
    private(set) var installing = false
    private var serviceChecked = false

    public let updates = Updates()

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
        updates.start()
    }

    /// A static model for screenshots: no daemon, no polling, no login item.
    public init(previewStatus: Status) {
        status = previewStatus
        connection = .connected
        openAtLogin = true
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
            await checkServiceVersion()
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

    // MARK: Service updates

    /// After the app updates itself, the service it bundles is newer than the one
    /// running. Updating it needs the administrator password, so ask right away
    /// only if the person started the update; otherwise offer a button.
    private func checkServiceVersion() async {
        guard !serviceChecked, !installing else { return }
        serviceChecked = true
        let prompt = UserDefaults.standard.bool(forKey: Updates.promptServiceUpdateKey)
        UserDefaults.standard.removeObject(forKey: Updates.promptServiceUpdateKey)
        guard case let .pong(running, _)? = try? await client.send(.ping), let runningVersion = Version(running),
            runningVersion < Updates.currentVersion
        else { return }
        serviceUpdateAvailable = true
        if prompt { updateService() }
    }

    /// Reinstall the service from the app bundle, keeping its configuration and
    /// tunnel key, through the standard macOS administrator prompt.
    func updateService() {
        guard let resources = Bundle.main.resourceURL,
            FileManager.default.fileExists(atPath: resources.appendingPathComponent("install.sh").path)
        else {
            actionError = "No se encuentra el instalador dentro de la app."
            return
        }
        installing = true
        let script = resources.appendingPathComponent("install.sh").path
        let command = "/bin/bash \(shellQuote(script)) --upgrade --bin-dir \(shellQuote(resources.path))"
        Task.detached {
            var error: NSDictionary?
            let source = "do shell script \"\(command.replacingOccurrences(of: "\"", with: "\\\""))\" with administrator privileges"
            NSAppleScript(source: source)?.executeAndReturnError(&error)
            let message = error?[NSAppleScript.errorMessage] as? String
            await MainActor.run {
                self.installing = false
                self.actionError = message
                if message == nil { self.serviceUpdateAvailable = false }
            }
            try? await Task.sleep(for: .seconds(1))
            await self.refresh()
        }
    }

    func openLog() {
        NSWorkspace.shared.open(URL(fileURLWithPath: "/var/log/macdetour.log"))
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

private func shellQuote(_ s: String) -> String {
    "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'"
}
