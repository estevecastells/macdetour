import DetourUI
import SwiftUI

@main
@MainActor
struct MacDetourApp: App {
    @State private var model = AppModel()

    var body: some Scene {
        MenuBarExtra {
            MenuContent(model: model)
        } label: {
            MenuBarLabel(status: model.status, connection: model.connection)
        }
        .menuBarExtraStyle(.window)
    }
}
