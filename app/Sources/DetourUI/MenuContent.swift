import AppKit
import DetourKit
import SwiftUI

public struct MenuBarLabel: View {
    let status: Status?
    let connection: Connection

    public init(status: Status?, connection: Connection) {
        self.status = status
        self.connection = connection
    }

    public var body: some View {
        Image(systemName: icon)
    }

    private var icon: String {
        guard connection == .connected, let status else { return "exclamationmark.triangle" }
        switch status.phase {
        case .off: return "arrow.triangle.branch"
        case .active: return "arrow.triangle.turn.up.right.circle.fill"
        case .connecting, .reconnecting: return "arrow.triangle.2.circlepath"
        }
    }
}

@MainActor
public struct MenuContent: View {
    @Bindable var model: AppModel

    public init(model: AppModel) {
        self.model = model
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            header
            Divider()
            switch model.connection {
            case .missing:
                SetupHint()
            case let .failed(message):
                Text(message).font(.callout).foregroundStyle(.red)
            case .connecting:
                ProgressView().controlSize(.small)
            case .connected:
                if let status = model.status { details(status) }
            }
            if let error = model.actionError {
                Text(error).font(.caption).foregroundStyle(.red).fixedSize(horizontal: false, vertical: true)
            }
            if model.serviceUpdateAvailable {
                HStack {
                    Label("Hay una actualización del servicio", systemImage: "arrow.down.circle.fill").font(.callout)
                    Spacer()
                    Button(model.installing ? "Actualizando…" : "Actualizar…") { model.updateService() }
                        .controlSize(.small)
                        .disabled(model.installing)
                        .help("Instala el servicio incluido en esta versión. macOS te pedirá la contraseña.")
                }
            }
            UpdateRow(updates: model.updates)
            Divider()
            VStack(alignment: .leading, spacing: 4) {
                Toggle("Abrir al iniciar sesión", isOn: Binding(get: { model.openAtLogin }, set: { model.setOpenAtLogin($0) }))
                Toggle("Instalar actualizaciones automáticamente", isOn: Binding(
                    get: { model.updates.autoInstall }, set: { model.updates.autoInstall = $0 }))
            }
            .toggleStyle(.checkbox)
            .font(.callout)
            HStack {
                Button("Log") { model.openLog() }
                Spacer()
                Button("Buscar actualizaciones") { Task { await model.updates.check(userInitiated: true) } }
                Spacer()
                Link("GitHub", destination: URL(string: "https://github.com/\(UpdateConfig.repo)")!)
                Spacer()
                Button("Salir") { NSApplication.shared.terminate(nil) }
            }
            .controlSize(.small)
        }
        .padding(14)
        .frame(width: 340)
        .onAppear { model.panelDidOpen() }
        .onDisappear { model.panelDidClose() }
    }

    private var header: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text("MacDetour").font(.headline)
                Text(Describe.phase(model.status)).font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            Toggle("", isOn: Binding(get: { model.isOn }, set: { model.setEnabled($0) }))
                .toggleStyle(.switch)
                .labelsHidden()
                .disabled(model.connection != .connected || model.busy)
        }
    }

    @ViewBuilder
    private func details(_ s: Status) -> some View {
        if s.enabled {
            row("Túnel", s.tunnel.connected ? "Conectado" : "Sin conexión", ok: s.tunnel.connected)
            row("Desvío", Describe.routes(s.routes), ok: nil)
            if !s.routes.sample.isEmpty {
                Text(s.routes.sample.joined(separator: ", "))
                    .font(.caption2.monospaced()).foregroundStyle(.secondary).lineLimit(2)
            }
            // The age text ticks on its own; the status itself rarely changes.
            TimelineView(.periodic(from: .now, by: 5)) { context in
                row(
                    "Lista",
                    s.list.refreshing
                        ? "Comprobando…"
                        : "\(s.list.listed) bloqueadas, \(Describe.ago(s.list.fetchedAt, now: context.date))",
                    ok: nil)
            }
            if let iface = s.primaryInterface { row("Red", iface, ok: nil) }
            HStack {
                Spacer()
                Button("Comprobar ahora") { model.refreshList() }.disabled(s.list.refreshing)
            }
        } else {
            Text("Actívalo para desviar por \(s.tunnel.server) solo las IPs que LaLiga bloquea en tu red. El resto de tu tráfico no cambia.")
                .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
        if let error = s.lastError {
            Text(error).font(.caption).foregroundStyle(.orange).fixedSize(horizontal: false, vertical: true)
        }
    }

    private func row(_ label: String, _ value: String, ok: Bool?) -> some View {
        HStack {
            Text(label).foregroundStyle(.secondary)
            Spacer()
            if let ok {
                Circle().fill(ok ? Color.green : Color.orange).frame(width: 7, height: 7)
            }
            Text(value).monospacedDigit()
        }
        .font(.callout)
    }
}

/// Shown when the service is missing: the one-line installer, ready to copy.
@MainActor
private struct SetupHint: View {
    static let command =
        "curl -fsSL https://raw.githubusercontent.com/\(UpdateConfig.repo)/main/scripts/get.sh | bash -s -- <tu-alias-ssh>"
    @State private var copied = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("El servicio de MacDetour no está instalado o no está en marcha. Instálalo desde Terminal con el alias SSH de tu servidor:")
                .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            Text(Self.command)
                .font(.caption2.monospaced()).textSelection(.enabled)
                .padding(6).background(.quaternary, in: RoundedRectangle(cornerRadius: 6))
            Button(copied ? "Copiado" : "Copiar comando") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(Self.command, forType: .string)
                copied = true
            }
            .controlSize(.small)
        }
    }
}

@MainActor
private struct UpdateRow: View {
    let updates: Updates

    var body: some View {
        switch updates.state {
        case .idle:
            EmptyView()
        case .checking:
            Label("Buscando actualizaciones…", systemImage: "arrow.triangle.2.circlepath").font(.caption).foregroundStyle(.secondary)
        case .upToDate:
            Label("MacDetour \(Updates.currentVersion) está al día", systemImage: "checkmark.circle")
                .font(.caption).foregroundStyle(.secondary)
        case let .available(version, notes):
            HStack {
                Label("Versión \(version) disponible", systemImage: "arrow.down.circle.fill").font(.callout)
                Spacer()
                Link("Novedades", destination: notes).font(.caption)
                Button("Actualizar") { Task { await updates.install() } }.controlSize(.small)
            }
        case let .installing(step):
            HStack(spacing: 6) {
                ProgressView().controlSize(.small)
                Text(step).font(.caption).foregroundStyle(.secondary)
            }
        case let .failed(message):
            Label(message, systemImage: "exclamationmark.triangle").font(.caption).foregroundStyle(.orange)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}
