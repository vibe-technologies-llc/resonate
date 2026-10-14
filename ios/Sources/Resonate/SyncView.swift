import ResonateNative
import SwiftUI
import UIKit

struct SyncView: View {
    @Environment(Library.self) private var library
    @State private var joining = ""
    @State private var device = ""
    @State private var server = ""
    private static let hostedServer = "resonate.tecnio.dev"

    var body: some View {
        let features = library.features
        let status = features.syncStatus
        Form {
            if let config = features.syncConfig {
                Section {
                    Toggle("Sync with your other devices", isOn: Binding(
                        get: { config.enabled },
                        set: { enabled in
                            var next = config
                            next.enabled = enabled
                            features.saveSync(next)
                        }
                    ))
                    Text(standing(status, online: features.online, enabled: config.enabled)).foregroundStyle(.secondary)
                    if !features.online {
                        Text("Turn on online features in Settings to sync.").foregroundStyle(.secondary)
                    }
                } footer: {
                    Text("Playlists, favourites, listening history and what is playing follow you between your devices. Records are sealed with your pairing code before they leave this iPhone.")
                }
                Section {
                    let code = status?.code.isEmpty == false ? status?.code : (config.key.isEmpty ? nil : config.key)
                    HStack {
                        Text(code ?? "Made when sync first starts")
                            .font(.body.monospaced())
                            .textSelection(.enabled)
                        Spacer()
                        if let code {
                            Button("Copy pairing code", systemImage: "doc.on.doc") { UIPasteboard.general.string = code }
                                .labelStyle(.iconOnly)
                        }
                    }
                    TextField("Join with a code from another device", text: $joining)
                        .textInputAutocapitalization(.characters)
                        .autocorrectionDisabled()
                    if !joining.isEmpty, !syncReadsAsACode(code: joining) {
                        Text("This is not a pairing code").foregroundStyle(.red)
                    }
                    Button("Join") {
                        features.joinSync(joining)
                        joining = ""
                    }
                    .disabled(!syncReadsAsACode(code: joining))
                    Button("Leave and make a new code", role: .destructive) { features.newSyncGroup() }
                } header: {
                    Text("Pairing code")
                } footer: {
                    Text("Enter this code on another device to join it to this one.")
                }
                Section("This device") {
                    TextField("Device name", text: $device, prompt: Text("iPhone"))
                    TextField("Sync server", text: $server, prompt: Text(Self.hostedServer))
                        .keyboardType(.URL)
                        .textInputAutocapitalization(.never)
                    Button("Save") {
                        var next = config
                        next.device = device.trimmingCharacters(in: .whitespaces)
                        next.server = server.trimmingCharacters(in: .whitespaces)
                        features.saveSync(next)
                    }
                }
                Section("Songs fetched from your devices") {
                    follow("Favourites", "Fetch every song favoured on another device", config.favourites) { $0.favourites = $1 }
                    follow("Kept playlists", "Fetch the songs of playlists kept on this device", config.playlists) { $0.playlists = $1 }
                    follow("Everything", "Fetch every song your other devices hold", config.everything) { $0.everything = $1 }
                    follow("Allow mobile data for songs", "Without it, songs move only on unmetered networks", config.meteredAllowed) { $0.meteredAllowed = $1 }
                }
                Section("Songs from your devices are kept in") {
                    let destinations = library.folderList.filter { $0.readable && $0.writable }
                    if destinations.isEmpty {
                        Text("Add a music folder with write access first.")
                    }
                    ForEach(destinations) { folder in
                        Button {
                            features.chooseSyncFolder(folder)
                        } label: {
                            HStack {
                                Text(folder.name)
                                Spacer()
                                if folder.id == (features.syncFolder ?? destinations.first?.id) {
                                    Image(systemName: "checkmark").foregroundStyle(.tint)
                                }
                            }
                        }
                        .buttonStyle(.plain)
                    }
                    if let notice = features.syncNotice {
                        Text(notice).foregroundStyle(.orange)
                    }
                }
                if let status, !status.devices.isEmpty {
                    Section("Devices") {
                        ForEach(status.devices, id: \.name) { known in
                            HStack {
                                Image(systemName: known.platform.symbol)
                                VStack(alignment: .leading) {
                                    Text(known.name + (known.thisDevice ? " (this iPhone)" : ""))
                                    if let platform = known.platform.title {
                                        Text(platform).font(.caption).foregroundStyle(.secondary)
                                    }
                                }
                                Spacer()
                                Circle()
                                    .fill(known.online ? Color.green : Color.secondary.opacity(0.4))
                                    .frame(width: 10, height: 10)
                                    .accessibilityLabel(known.online ? "Online" : "Offline")
                            }
                        }
                        if status.moving > 0 {
                            Text("Moving \(counted(Int(status.moving), "song")) between your devices")
                        }
                        HandoffRows()
                    }
                }
            }
        }
        .navigationTitle("Device sync")
        .onAppear {
            device = features.syncConfig?.device ?? ""
            server = features.syncConfig?.server ?? ""
            library.folderList = library.folders.folders()
        }
    }

    private func follow(
        _ title: String,
        _ detail: String,
        _ value: Bool,
        _ apply: @escaping (inout SyncConfig, Bool) -> Void
    ) -> some View {
        Toggle(isOn: Binding(get: { value }, set: { chosen in
            guard var next = library.features.syncConfig else { return }
            apply(&next, chosen)
            library.features.saveSync(next)
        })) {
            VStack(alignment: .leading) {
                Text(title)
                Text(detail).font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private func standing(_ status: SyncStatus?, online: Bool, enabled: Bool) -> String {
        guard enabled else { return "Off" }
        guard online else { return "Waiting for online features" }
        switch status?.standing {
        case .connected:
            let others = status?.devices.filter { $0.online && !$0.thisDevice }.count ?? 0
            return others == 0 ? "Connected · no other device online" : "Connected · \(counted(others, "device")) online"
        case .unreachable: return "The sync server could not be reached"
        case .refused: return "The sync server refused this device"
        case .connecting: return "Connecting…"
        default: return "Starting…"
        }
    }
}

extension DevicePlatform {
    var title: String? {
        switch self {
        case .linux: "Linux"
        case .android: "Android"
        case .ios: "iOS"
        case .other: nil
        }
    }

    var symbol: String {
        switch self {
        case .linux: "desktopcomputer"
        case .android, .ios: "iphone"
        case .other: "questionmark.square.dashed"
        }
    }
}
