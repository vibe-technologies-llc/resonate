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
    @State private var confirmingRegenerate = false
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
                    if config.enabled {
                        SyncStateRow()
                        Button {
                            features.forceSync()
                        } label: {
                            Label(features.syncAskedAt == nil ? "Sync now" : "Syncing…", systemImage: "arrow.triangle.2.circlepath")
                                .symbolEffect(.rotate, options: .repeat(.continuous), isActive: features.syncAskedAt != nil)
                        }
                        .disabled(!features.online || features.syncAskedAt != nil)
                    }
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
                    Button("Join") {
                        if code == nil { join() } else { confirmingJoin = true }
                    }
                    .disabled(!syncReadsAsACode(code: joining))
                    Button("Regenerate pairing code", systemImage: "arrow.clockwise", role: .destructive) {
                        confirmingRegenerate = true
                    }
                } header: {
                    Text("Pairing code")
                } footer: {
                    Text("Enter this code on another device to join it to this one. Regenerate it if someone else may have seen it.")
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
                if let status, !status.transfers.isEmpty {
                    SyncTransfers(transfers: status.transfers)
                }
                if let status, !status.devices.isEmpty {
                    let offline = status.devices.filter { !$0.online && !$0.thisDevice }
                    Section {
                        ForEach(status.devices, id: \.id) { known in
                            KnownDeviceRow(known: known)
                                .swipeActions(edge: .trailing) {
                                    if !known.online, !known.thisDevice {
                                        Button("Remove", systemImage: "trash", role: .destructive) {
                                            features.forgetSyncDevices([known])
                                        }
                                    }
                                }
                                .contextMenu {
                                    if !known.online, !known.thisDevice {
                                        Button("Remove device", systemImage: "trash", role: .destructive) {
                                            features.forgetSyncDevices([known])
                                        }
                                    }
                                }
                        }
                        if offline.count > 1 {
                            Button("Remove offline devices", systemImage: "trash", role: .destructive) {
                                features.forgetSyncDevices(offline)
                            }
                        }
                        HandoffRows()
                    } header: {
                        Text("Devices")
                    } footer: {
                        if !offline.isEmpty {
                            Text("Swipe an offline device to remove it from every device's list. A removed device that syncs again comes back.")
                        }
                    }
                }
                SyncDestinations()
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
        .confirmationDialog("Regenerate the pairing code?", isPresented: $confirmingRegenerate, titleVisibility: .visible) {
            Button("Regenerate pairing code", role: .destructive) { features.newSyncGroup() }
        } message: {
            Text("Your other devices stop syncing with this iPhone until you enter the new code on them. Playlists, favourites and history stay here and are sent again under the new code.")
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

struct SyncStateRow: View {
    @Environment(Library.self) private var library

    var body: some View {
        let features = library.features
        let status = features.syncStatus
        HStack(spacing: 12) {
            Group {
                if features.syncActivity != nil || status?.standing == .connecting {
                    ProgressView()
                } else if features.syncUpToDate {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
                } else if status?.standing == .unreachable || status?.standing == .refused {
                    Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                } else {
                    Image(systemName: "clock").foregroundStyle(.secondary)
                }
            }
            .frame(width: 24)
            .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(headline(features))
                Text(syncStanding(status, config: features.syncConfig, online: features.online))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
        .animation(.smooth, value: headline(features))
    }

    private func headline(_ features: Features) -> String {
        switch features.syncActivity {
        case .fetching(let songs, _): return "Fetching \(counted(songs, "song"))"
        case .syncing: return "Syncing…"
        case nil: break
        }
        guard let status = features.syncStatus else { return "Starting…" }
        if features.syncUpToDate { return "Up to date" }
        switch status.standing {
        case .connecting: return "Connecting…"
        case .refused: return "Refused"
        case .unreachable: return status.devices.contains { $0.nearby && !$0.thisDevice } ? "Catching up" : "Not syncing"
        case .connected: return status.owed > 0 ? "Sending \(counted(Int(status.owed), "change"))" : "Catching up"
        case .off: return "Off"
        }
    }
}

struct SyncTransfers: View {
    let transfers: [SyncTransfer]
    private static let finishedShown = 10

    var body: some View {
        let active = transfers.filter { $0.state == .waiting || $0.state == .moving }
        let finished = transfers.filter { $0.state == .landed || $0.state == .failed }.suffix(Self.finishedShown).reversed()
        Section {
            ForEach(Array(active.enumerated()), id: \.offset) { _, transfer in
                SyncTransferRow(transfer: transfer)
            }
            ForEach(Array(finished.enumerated()), id: \.offset) { _, transfer in
                SyncTransferRow(transfer: transfer)
            }
        } header: {
            Text(active.isEmpty ? "Recently fetched" : "Fetching")
        }
    }
}

struct SyncTransferRow: View {
    let transfer: SyncTransfer

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                Image(systemName: symbol)
                    .foregroundStyle(tint)
                    .frame(width: 20)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 1) {
                    Text(transfer.title.isEmpty ? "Unknown song" : transfer.title).lineLimit(1)
                    Text(caption)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .monospacedDigit()
                }
            }
            if transfer.state == .moving, transfer.size > 0 {
                ProgressView(value: Double(min(transfer.received, transfer.size)), total: Double(transfer.size))
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var symbol: String {
        switch transfer.state {
        case .waiting: "clock"
        case .moving: "arrow.down.circle"
        case .landed: "checkmark.circle.fill"
        case .failed: "exclamationmark.circle.fill"
        }
    }

    private var tint: Color {
        switch transfer.state {
        case .waiting: .secondary
        case .moving: .accentColor
        case .landed: .green
        case .failed: .orange
        }
    }

    private var caption: String {
        let source = [transfer.artist, transfer.from.isEmpty ? nil : "from \(transfer.from)"]
            .compactMap { $0?.isEmpty == false ? $0 : nil }
            .joined(separator: " · ")
        let state: String = switch transfer.state {
        case .waiting: "Waiting"
        case .moving where transfer.size > 0:
            "\(ByteCountFormatter.string(fromByteCount: Int64(transfer.received), countStyle: .file)) of \(ByteCountFormatter.string(fromByteCount: Int64(transfer.size), countStyle: .file))"
        case .moving: "Fetching"
        case .landed: "Fetched"
        case .failed: "Could not be fetched"
        }
        return source.isEmpty ? state : "\(state) · \(source)"
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
