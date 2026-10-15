import ResonateNative
import SwiftUI
import UIKit

func syncStanding(_ status: SyncStatus?, config: SyncConfig?, online: Bool) -> String {
    guard let config, config.enabled else { return "Off" }
    if !online, !config.nearbyOnly { return "Waiting for online features" }
    let others = status?.devices.filter { !$0.thisDevice } ?? []
    let nearby = others.filter(\.nearby).count
    let reached = others.filter(\.online).count
    switch status?.standing {
    case .connected:
        return reached == 0 ? "No other device online" : "\(counted(reached, "device")) online"
    case .nearby:
        return nearby == 0 ? "Looking on this network" : "\(counted(nearby, "device")) on this network"
    case .unreachable:
        return nearby == 0 ? "Can't reach the server" : "Server unreachable · \(counted(nearby, "device")) on this network"
    case .refused:
        return "The server refused this iPhone"
    case .connecting:
        return nearby == 0 ? "Connecting…" : "Connecting · \(counted(nearby, "device")) on this network"
    case .off, nil:
        return "Starting…"
    }
}

enum SyncRoute: Hashable {
    case hosted
    case own
    case nearby

    init(_ config: SyncConfig) {
        if config.nearbyOnly {
            self = .nearby
        } else if config.server.trimmingCharacters(in: .whitespaces).isEmpty {
            self = .hosted
        } else {
            self = .own
        }
    }
}

struct SyncView: View {
    @Environment(Library.self) private var library
    @State private var pairing = false

    var body: some View {
        let features = library.features
        ThemedForm {
            if let config = features.syncConfig {
                Section {
                    Toggle("Sync this iPhone", isOn: Binding(
                        get: { config.enabled },
                        set: { enabled in
                            guard var next = library.features.syncConfig else { return }
                            next.enabled = enabled
                            library.features.saveSync(next)
                        }
                    ))
                } footer: {
                    Text("Playlists, favourites, history and what's playing follow you between your devices, sealed with your pairing code.")
                }
                if config.enabled {
                    SyncStatusSection()
                    SyncDevices(pairing: $pairing)
                    if !features.syncHandoffs.isEmpty {
                        Section("Continue listening") {
                            HandoffRows()
                        }
                    }
                    if let status = features.syncStatus, !status.transfers.isEmpty {
                        SyncTransfers(transfers: status.transfers)
                    }
                    Section("Options") {
                        NavigationLink {
                            SyncSongsPage()
                        } label: {
                            LabeledContent {
                                Text(Self.songs(config))
                            } label: {
                                Label("Songs", systemImage: "music.note")
                            }
                        }
                        NavigationLink {
                            SyncConnectionPage()
                        } label: {
                            LabeledContent {
                                Text(Self.connection(config))
                            } label: {
                                Label("Connection", systemImage: "point.3.connected.trianglepath.dotted")
                            }
                        }
                    }
                }
            }
        }
        .navigationTitle("Device sync")
        .sheet(isPresented: $pairing) {
            PairingSheet()
        }
        .task { library.folderList = await library.folders.refreshed() }
    }

    private static func songs(_ config: SyncConfig) -> String {
        if config.everything { return "Everything" }
        let kinds = [config.favourites ? "Favourites" : nil, config.playlists ? "Kept playlists" : nil].compactMap { $0 }
        return kinds.isEmpty ? "When asked" : kinds.joined(separator: ", ")
    }

    private static func connection(_ config: SyncConfig) -> String {
        switch SyncRoute(config) {
        case .hosted: "Resonate server"
        case .own: config.server.trimmingCharacters(in: .whitespaces)
        case .nearby: "This network only"
        }
    }
}

struct SyncStatusSection: View {
    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        let features = library.features
        let offline = !features.online && features.syncConfig?.nearbyOnly != true
        Section {
            SyncStateRow()
            if offline {
                Button("Sync only on this network", systemImage: "wifi") {
                    guard var next = features.syncConfig else { return }
                    next.nearbyOnly = true
                    features.saveSync(next)
                }
            } else {
                Button {
                    features.forceSync()
                } label: {
                    Label(features.syncAskedAt == nil ? "Sync now" : "Syncing…", systemImage: "arrow.triangle.2.circlepath")
                        .symbolEffect(.rotate, options: .repeat(.continuous), isActive: features.syncAskedAt != nil && !reduceMotion)
                }
                .disabled(!features.syncRuns || features.syncAskedAt != nil)
            }
        } footer: {
            if offline {
                Text("Online features are off. Turn them on in Settings, or sync only with devices on the same network.")
            }
        }
    }
}

struct SyncStateRow: View {
    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

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
                } else if status?.standing == .nearby {
                    Image(systemName: "wifi").foregroundStyle(.secondary)
                } else {
                    Image(systemName: "clock").foregroundStyle(.secondary)
                }
            }
            .font(.title3)
            .frame(width: 28)
            .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(headline(features)).font(.headline)
                Text(syncStanding(status, config: features.syncConfig, online: features.online))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
        .motion(.smooth, value: headline(features))
    }

    private func headline(_ features: Features) -> String {
        switch features.syncActivity {
        case .fetching(let songs, _): return "Fetching \(counted(songs, "song"))"
        case .syncing: return "Syncing…"
        case nil: break
        }
        guard let status = features.syncStatus else { return "Starting…" }
        if features.syncUpToDate { return "Up to date" }
        let nearby = status.devices.contains { $0.nearby && !$0.thisDevice }
        switch status.standing {
        case .connecting: return "Connecting…"
        case .refused: return "Refused"
        case .unreachable: return nearby ? "Catching up" : "Not syncing"
        case .nearby: return nearby ? "Catching up" : "Looking for devices"
        case .connected: return status.owed > 0 ? "Sending \(counted(Int(status.owed), "change"))" : "Catching up"
        case .off: return "Off"
        }
    }
}

struct SyncDevices: View {
    @Environment(Library.self) private var library
    @Binding var pairing: Bool

    var body: some View {
        let features = library.features
        let devices = features.syncStatus?.devices ?? []
        let others = devices.filter { !$0.thisDevice }.sorted { first, second in
            first.online != second.online ? first.online : first.name.localizedStandardCompare(second.name) == .orderedAscending
        }
        let away = others.filter { !$0.online }
        Section {
            ForEach(devices.filter(\.thisDevice) + others, id: \.id) { known in
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
            if others.isEmpty {
                Text("No other devices yet. Add one with this iPhone's pairing code.")
                    .foregroundStyle(.secondary)
            }
            Button("Add a device…", systemImage: "plus.circle") {
                pairing = true
            }
        } header: {
            HStack {
                Text("Devices")
                Spacer()
                if away.count > 1 {
                    Menu {
                        Button("Remove \(counted(away.count, "device")) that are away", systemImage: "trash", role: .destructive) {
                            features.forgetSyncDevices(away)
                        }
                    } label: {
                        Image(systemName: "ellipsis.circle")
                            .accessibilityLabel("Device options")
                    }
                }
            }
        } footer: {
            if !away.isEmpty {
                Text("Swipe a device that's away to remove it. A removed device that syncs again comes back.")
            }
        }
    }
}

struct KnownDeviceRow: View {
    let known: SyncedDevice

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: known.platform.symbol)
                .font(.title3)
                .foregroundStyle(known.online ? Color.worn : Color.secondary)
                .frame(width: 28)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(known.name)
                Text(caption)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            if !known.thisDevice {
                Circle()
                    .fill(known.online ? Color.green : Color.secondary.opacity(0.35))
                    .frame(width: 8, height: 8)
                    .accessibilityHidden(true)
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var caption: String {
        let state = if known.thisDevice {
            "This iPhone"
        } else if known.nearby {
            "On this network"
        } else if known.online {
            "Online"
        } else {
            "Away"
        }
        return [known.platform.title, state].compactMap { $0 }.joined(separator: " · ")
    }
}

struct PairingSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.dismiss) private var dismiss
    @State private var joining = ""
    @State private var confirmingJoin = false
    @State private var confirmingNew = false
    @State private var copies = 0
    @State private var revealed = false

    var body: some View {
        let code = shownCode
        let reads = syncReadsAsACode(code: joining)
        NavigationStack {
            Form {
                Section {
                    VStack(spacing: 16) {
                        if let code {
                            Text(code)
                                .font(.title3.monospaced().weight(.semibold))
                                .multilineTextAlignment(.center)
                                .textSelection(.enabled)
                                .frame(maxWidth: .infinity)
                                .blur(radius: revealed ? 0 : 9)
                                .privacySensitive()
                                .overlay {
                                    if !revealed {
                                        Label("Tap to show", systemImage: "eye")
                                            .font(.subheadline.weight(.semibold))
                                            .padding(.horizontal, 14)
                                            .padding(.vertical, 8)
                                            .glassEffect(.regular, in: .capsule)
                                    }
                                }
                                .contentShape(Rectangle())
                                .onTapGesture { withAnimation(reduceMotion ? nil : .smooth) { revealed.toggle() } }
                                .accessibilityLabel(revealed ? code : "Pairing code, hidden")
                                .accessibilityHint(revealed ? "Hides the code" : "Shows the code")
                                .accessibilityAddTraits(.isButton)
                        } else {
                            Text("Made when sync first starts")
                                .foregroundStyle(.secondary)
                                .frame(maxWidth: .infinity)
                        }
                        if let code {
                            HStack(spacing: 12) {
                                Button {
                                    UIPasteboard.general.string = code
                                    copies += 1
                                } label: {
                                    Label("Copy", systemImage: "doc.on.doc")
                                }
                                ShareLink(item: code, subject: Text("Resonate pairing code")) {
                                    Label("Share", systemImage: "square.and.arrow.up")
                                }
                            }
                            .buttonStyle(.bordered)
                        }
                    }
                    .padding(.vertical, 8)
                } header: {
                    Text("This iPhone's code")
                } footer: {
                    Text("On your other device, open Device sync and enter this code. To pair with a computer, scan the QR code under its Settings › Sync with the Camera app.")
                }
                Section {
                    TextField("Pairing code", text: $joining)
                        .font(.body.monospaced())
                        .textInputAutocapitalization(.characters)
                        .autocorrectionDisabled()
                    if !joining.isEmpty, !reads {
                        Text("This is not a pairing code")
                            .font(.caption)
                            .foregroundStyle(.red)
                    }
                    Button("Join") {
                        if code == nil { join() } else { confirmingJoin = true }
                    }
                    .disabled(!reads)
                } header: {
                    Text("Use another device's code")
                } footer: {
                    Text("This iPhone then syncs with the devices that share that code.")
                }
                Section {
                    Button("Make a new code…", role: .destructive) {
                        confirmingNew = true
                    }
                } footer: {
                    Text("Do this if someone else may have seen the code. Your other devices then need the new one.")
                }
            }
            .navigationTitle("Add a device")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .sensoryFeedback(.success, trigger: copies)
            .confirmationDialog("Join another group?", isPresented: $confirmingJoin, titleVisibility: .visible) {
                Button("Join") { join() }
            } message: {
                Text("This iPhone leaves the devices it syncs with now and syncs with the devices that share this code.")
            }
            .confirmationDialog("Make a new pairing code?", isPresented: $confirmingNew, titleVisibility: .visible) {
                Button("Make a new code", role: .destructive) { library.features.newSyncGroup() }
            } message: {
                Text("Your other devices stop syncing with this iPhone until you enter the new code on them. Playlists, favourites and history stay here and are sent again under the new code.")
            }
        }
    }

    private var shownCode: String? {
        let features = library.features
        if let code = features.syncStatus?.code, !code.isEmpty { return code }
        guard let key = features.syncConfig?.key, !key.isEmpty else { return nil }
        return key
    }

    private func join() {
        library.features.joinSync(joining)
        joining = ""
        dismiss()
    }
}

struct SyncSwitch: View {
    @Environment(Library.self) private var library
    let title: String
    let detail: String
    let choice: WritableKeyPath<SyncConfig, Bool>

    var body: some View {
        Toggle(isOn: Binding(
            get: { library.features.syncConfig?[keyPath: choice] ?? false },
            set: { chosen in
                guard var next = library.features.syncConfig else { return }
                next[keyPath: choice] = chosen
                library.features.saveSync(next)
            }
        )) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                Text(detail).font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}

struct SyncSongsPage: View {
    @Environment(Library.self) private var library

    var body: some View {
        let features = library.features
        let destinations = library.folderList.filter { $0.readable && $0.writable }
        ThemedForm {
            Section {
                SyncSwitch(title: "Favourites", detail: "Songs favoured on another device", choice: \.favourites)
                SyncSwitch(title: "Kept playlists", detail: "Playlists set to keep their songs here", choice: \.playlists)
                SyncSwitch(title: "Everything", detail: "Every song your other devices hold", choice: \.everything)
            } header: {
                Text("Fetch automatically")
            } footer: {
                Text("A playlist's menu can always fetch its missing songs from a device.")
            }
            Section {
                SyncSwitch(title: "Use mobile data", detail: "Without it, songs move only on Wi-Fi", choice: \.meteredAllowed)
            }
            Section {
                if let first = destinations.first {
                    let kept = destinations.first { $0.id == features.syncFolder } ?? first
                    Picker("Keep songs in", selection: Binding(
                        get: { kept.id },
                        set: { chosen in
                            guard let folder = destinations.first(where: { $0.id == chosen }) else { return }
                            features.chooseSyncFolder(folder)
                        }
                    )) {
                        ForEach(destinations) { folder in
                            Text(folder.name).tag(folder.id)
                        }
                    }
                } else {
                    ContentUnavailableView(
                        "No folder to keep songs in",
                        systemImage: "folder.badge.questionmark",
                        description: Text("Add a music folder with write access first.")
                    )
                }
                if let notice = features.syncNotice {
                    Label(notice, systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                }
            } footer: {
                Text("Songs from your other devices are saved in this folder and added to your library.")
            }
        }
        .navigationTitle("Songs")
        .task { library.folderList = await library.folders.refreshed() }
    }
}

struct SyncConnectionPage: View {
    @Environment(Library.self) private var library
    @State private var chosen: SyncRoute?
    @State private var server = ""
    @State private var device = ""

    var body: some View {
        let features = library.features
        ThemedForm {
            if let config = features.syncConfig {
                let route = chosen ?? SyncRoute(config)
                Section {
                    Picker("Sync through", selection: Binding(get: { route }, set: { choose($0, config) })) {
                        Label("Resonate's server", systemImage: "cloud").tag(SyncRoute.hosted)
                        Label("Your own server", systemImage: "server.rack").tag(SyncRoute.own)
                        Label("This network only", systemImage: "wifi").tag(SyncRoute.nearby)
                    }
                    .pickerStyle(.inline)
                    .labelsHidden()
                } header: {
                    Text("Sync through")
                } footer: {
                    Text(Self.explanation(route))
                }
                if route == .own {
                    Section {
                        TextField("host or host:port", text: $server)
                            .keyboardType(.URL)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                        Button("Use this server") {
                            guard var next = features.syncConfig else { return }
                            next.server = trimmed(server)
                            next.nearbyOnly = false
                            features.saveSync(next)
                        }
                        .disabled(trimmed(server).isEmpty || (trimmed(server) == config.server && !config.nearbyOnly))
                    } header: {
                        Text("Server address")
                    }
                }
                Section {
                    TextField("Name", text: $device, prompt: Text("iPhone"))
                    Button("Save name") {
                        guard var next = features.syncConfig else { return }
                        next.device = trimmed(device)
                        features.saveSync(next)
                    }
                    .disabled(trimmed(device) == config.device)
                } header: {
                    Text("This iPhone's name")
                } footer: {
                    Text("What your other devices call this iPhone.")
                }
            }
        }
        .navigationTitle("Connection")
        .onAppear {
            guard let config = features.syncConfig else { return }
            server = config.server
            device = config.device
        }
    }

    private func choose(_ route: SyncRoute, _ config: SyncConfig) {
        chosen = route
        var next = config
        switch route {
        case .hosted:
            next.nearbyOnly = false
            next.server = ""
        case .nearby:
            next.nearbyOnly = true
        case .own:
            guard !config.server.trimmingCharacters(in: .whitespaces).isEmpty else { return }
            next.nearbyOnly = false
        }
        if next != config { library.features.saveSync(next) }
    }

    private func trimmed(_ text: String) -> String {
        text.trimmingCharacters(in: .whitespaces)
    }

    private static func explanation(_ route: SyncRoute) -> String {
        switch route {
        case .hosted:
            "Your devices meet through Resonate's server wherever they are, and directly on the same network. Everything is sealed with your pairing code, so the server can't read it."
        case .own:
            "Your devices meet through a resonate-server you run, and directly on the same network."
        case .nearby:
            "Your devices sync only when they're on the same Wi-Fi. Nothing leaves your network, and it works with online features off."
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
        case .moving: Color.worn
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
