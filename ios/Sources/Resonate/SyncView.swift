import ResonateNative
import SwiftUI
import UIKit

func syncStanding(_ status: SyncStatus?, config: SyncConfig?, online: Bool) -> String {
    guard config?.enabled == true else { return "Off" }
    guard online else { return "Waiting for online features" }
    switch status?.standing {
    case .connected:
        let others = status?.devices.filter { $0.online && !$0.thisDevice }.count ?? 0
        return others == 0 ? "Connected · no other device online" : "Connected · \(counted(others, "device")) online"
    case .unreachable:
        let nearby = status?.devices.filter { $0.nearby && !$0.thisDevice }.count ?? 0
        return nearby == 0 ? "The sync server could not be reached" : "Server unreachable · \(counted(nearby, "device")) on this network"
    case .refused: return "The sync server refused this device"
    case .connecting:
        let nearby = status?.devices.filter { $0.nearby && !$0.thisDevice }.count ?? 0
        return nearby == 0 ? "Connecting…" : "Connecting… · \(counted(nearby, "device")) on this network"
    default: return "Starting…"
    }
}

struct SyncView: View {
    @Environment(Library.self) private var library
    @State private var joining = ""
    @State private var device = ""
    @State private var server = ""
    @State private var confirmingJoin = false
    @State private var confirmingLeave = false
    @State private var copies = 0
    private static let hostedServer = "resonate.tecnio.dev"

    var body: some View {
        let features = library.features
        let status = features.syncStatus
        Form {
            if let config = features.syncConfig {
                let code = status?.code.isEmpty == false ? status?.code : (config.key.isEmpty ? nil : config.key)
                Section {
                    Toggle("Sync with your other devices", isOn: Binding(
                        get: { config.enabled },
                        set: { enabled in
                            var next = config
                            next.enabled = enabled
                            features.saveSync(next)
                        }
                    ))
                    Text(syncStanding(status, config: config, online: features.online)).foregroundStyle(.secondary)
                    if !features.online {
                        Text("Turn on online features in Settings to sync.").foregroundStyle(.secondary)
                    }
                } footer: {
                    Text("Playlists, favourites, listening history and what is playing follow you between your devices. Records are sealed with your pairing code before they leave this iPhone.")
                }
                Section {
                    PairingCode(code: code, copies: $copies)
                    TextField("Join with a code from another device", text: $joining)
                        .textInputAutocapitalization(.characters)
                        .autocorrectionDisabled()
                    if !joining.isEmpty, !syncReadsAsACode(code: joining) {
                        Text("This is not a pairing code").foregroundStyle(.red)
                    }
                    Button("Sync now", systemImage: "arrow.triangle.2.circlepath") { features.forceSync() }
                        .disabled(!config.enabled || !features.online)
                    Button("Join") {
                        if code == nil { join() } else { confirmingJoin = true }
                    }
                    .disabled(!syncReadsAsACode(code: joining))
                    Button("Leave and make a new code", role: .destructive) { confirmingLeave = true }
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
                        next.device = trimmed(device)
                        next.server = trimmed(server)
                        features.saveSync(next)
                    }
                    .disabled(trimmed(device) == config.device && trimmed(server) == config.server)
                }
                Section("Songs fetched from your devices") {
                    follow("Favourites", "Fetch every song favoured on another device", config.favourites) { $0.favourites = $1 }
                    follow("Kept playlists", "Fetch the songs of playlists kept on this device", config.playlists) { $0.playlists = $1 }
                    follow("Everything", "Fetch every song your other devices hold", config.everything) { $0.everything = $1 }
                    follow("Allow mobile data for songs", "Without it, songs move only on unmetered networks", config.meteredAllowed) { $0.meteredAllowed = $1 }
                }
                SyncDestinations()
                if let status, !status.devices.isEmpty {
                    Section("Devices") {
                        ForEach(status.devices, id: \.name) { known in
                            KnownDeviceRow(known: known)
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
        .sensoryFeedback(.success, trigger: copies)
        .task { library.folderList = await library.folders.refreshed() }
        .onChange(of: features.syncConfig?.device, initial: true) { saved, now in
            if trimmed(device) == (saved ?? "") { device = now ?? "" }
        }
        .onChange(of: features.syncConfig?.server, initial: true) { saved, now in
            if trimmed(server) == (saved ?? "") { server = now ?? "" }
        }
        .confirmationDialog("Join another group?", isPresented: $confirmingJoin, titleVisibility: .visible) {
            Button("Join") { join() }
        } message: {
            Text("This iPhone leaves the devices it syncs with now and syncs with the devices that share this code.")
        }
        .confirmationDialog("Leave and make a new code?", isPresented: $confirmingLeave, titleVisibility: .visible) {
            Button("Leave and make a new code", role: .destructive) { features.newSyncGroup() }
        } message: {
            Text("This iPhone stops syncing with your other devices until they join its new code.")
        }
    }

    private func join() {
        library.features.joinSync(joining)
        joining = ""
    }

    private func trimmed(_ text: String) -> String {
        text.trimmingCharacters(in: .whitespaces)
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
}

struct PairingCode: View {
    let code: String?
    @Binding var copies: Int

    var body: some View {
        HStack {
            Text(code ?? "Made when sync first starts")
                .font(.body.monospaced())
                .textSelection(.enabled)
            Spacer()
            if let code {
                Button("Copy pairing code", systemImage: "doc.on.doc") {
                    UIPasteboard.general.string = code
                    copies += 1
                }
                ShareLink(item: code, subject: Text("Resonate pairing code")) {
                    Label("Share pairing code", systemImage: "square.and.arrow.up")
                }
            }
        }
        .labelStyle(.iconOnly)
        .buttonStyle(.borderless)
    }
}

struct SyncDestinations: View {
    @Environment(Library.self) private var library

    var body: some View {
        let features = library.features
        let destinations = library.folderList.filter { $0.readable && $0.writable }
        Section("Songs from your devices are kept in") {
            if destinations.isEmpty {
                ContentUnavailableView(
                    "No folder to keep songs in",
                    systemImage: "folder.badge.questionmark",
                    description: Text("Add a music folder with write access first.")
                )
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
    }
}

struct KnownDeviceRow: View {
    let known: SyncedDevice

    var body: some View {
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
        .accessibilityElement(children: .combine)
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
