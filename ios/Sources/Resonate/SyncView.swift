import ResonateLogic
import ResonateNative
import SwiftUI
import UIKit
import UniformTypeIdentifiers

struct SyncView: View {
    @Environment(Library.self) private var library
    @State private var pairing = false

    var body: some View {
        let features = library.features
        ThemedForm {
            if let config = features.syncConfig {
                Section {
                    Toggle("sync.enabled", isOn: Binding(
                        get: { config.enabled },
                        set: { enabled in
                            guard var next = library.features.syncConfig else { return }
                            next.enabled = enabled
                            library.features.saveSync(next)
                        }
                    ))
                } footer: {
                    Text("sync.enabled.footer")
                }
                if config.enabled {
                    SyncStatusSection()
                    SyncDevices(pairing: $pairing)
                    if !features.syncHandoffs.isEmpty {
                        Section("sync.continue-listening") {
                            HandoffRows()
                        }
                    }
                    if let status = features.syncStatus, !status.transfers.isEmpty {
                        SyncTransfers(
                            transfers: status.transfers,
                            downloading: status.downloading,
                            waiting: status.waiting
                        )
                    }
                    Section("sync.options") {
                        NavigationLink(value: Destination.syncSongs) {
                            ToolRow(title: said("sync.options.songs"), detail: Self.songs(config), symbol: SettingsPlace.syncSongs.symbol)
                        }
                        NavigationLink(value: Destination.syncConnection) {
                            ToolRow(title: said("sync.options.connection"), detail: Self.connection(config), symbol: SettingsPlace.syncConnection.symbol)
                        }
                        NavigationLink(value: Destination.syncControl) {
                            let offered = SyncControlsOffered.shown
                            ToolRow(
                                title: offered.title,
                                detail: offered.choices.first { $0.control == config.control }?.label ?? "",
                                symbol: SettingsPlace.syncControl.symbol
                            )
                        }
                    }
                }
            } else {
                Section {
                    Label("sync.unread", systemImage: "exclamationmark.triangle")
                    Button("common.try-again", systemImage: "arrow.clockwise") { features.readSecretsAgain() }
                } footer: {
                    Text("sync.unread.footer")
                }
            }
        }
        .navigationTitle("sync.title")
        .sheet(isPresented: $pairing) {
            PairingSheet().sheetSized()
        }
        .task { library.folderList = await library.folders.refreshed() }
    }

    private static func songs(_ config: SyncConfig) -> String {
        if config.everything { return said("sync.songs.everything") }
        let kinds = [config.favourites ? said("sync.songs.favourites") : nil, config.playlists ? said("sync.songs.kept-playlists") : nil].compactMap { $0 }
        return kinds.isEmpty ? said("sync.songs.when-asked") : kinds.joined(separator: ", ")
    }

    private static func connection(_ config: SyncConfig) -> String {
        switch SyncRoute(config) {
        case .hosted: said("sync.route.resonate-server")
        case .own: config.server.trimmingCharacters(in: .whitespaces)
        case .nearby: said("sync.route.nearby")
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
            if features.localNetworkRefused {
                LocalNetworkRefusedRow()
            }
            if offline {
                Button("sync.only-nearby", systemImage: "wifi") {
                    guard var next = features.syncConfig else { return }
                    next.nearbyOnly = true
                    features.saveSync(next)
                }
            } else {
                Button {
                    features.forceSync()
                } label: {
                    Label(said(features.syncAskedAt == nil ? "sync.now" : "sync.syncing"), systemImage: "arrow.triangle.2.circlepath")
                        .symbolEffect(.rotate, options: .repeat(.continuous), isActive: features.syncAskedAt != nil && !reduceMotion)
                }
                .disabled(!features.syncRuns || features.syncAskedAt != nil)
            }
        } footer: {
            if offline {
                Text("sync.offline.footer")
            }
        }
    }
}

struct LocalNetworkRefusedRow: View {
    @Environment(\.openURL) private var openURL

    var body: some View {
        Label {
            VStack(alignment: .leading, spacing: 2) {
                Text("sync.local-network.refused")
                Text("sync.local-network.refused.detail")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        } icon: {
            Image(systemName: "network.slash").foregroundStyle(Color.warning).accessibilityHidden(true)
        }
        .padding(.vertical, 4)
        Button("common.open-settings", systemImage: "gear") {
            if let settings = URL(string: UIApplication.openSettingsURLString) { openURL(settings) }
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
                    ProgressView().accessibilityHidden(true)
                } else if features.syncUpToDate {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.confirmation)
                } else if status?.standing == .unreachable || status?.standing == .refused {
                    Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(Color.warning)
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
        case .fetching(let songs, _): return said("sync.state.fetching", songs)
        case .syncing: return said("sync.syncing")
        case nil: break
        }
        guard let status = features.syncStatus else { return said("sync.state.starting") }
        if features.syncUpToDate { return said("sync.state.up-to-date") }
        let nearby = status.devices.contains { $0.nearby && !$0.thisDevice }
        switch status.standing {
        case .connecting: return said("sync.state.connecting")
        case .refused: return said("sync.state.refused")
        case .unreachable: return said(nearby ? "sync.state.catching-up" : "sync.state.not-syncing")
        case .nearby: return said(nearby ? "sync.state.catching-up" : "sync.state.looking")
        case .connected: return status.owed > 0 ? said("sync.state.sending", Int(status.owed)) : said("sync.state.catching-up")
        case .off: return said("sync.state.off")
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
                KnownDeviceEntry(known: known)
                    .swipeActions(edge: .trailing) {
                        if !known.online, !known.thisDevice {
                            Button("common.remove", systemImage: "trash", role: .destructive) {
                                features.forgetSyncDevices([known])
                            }
                            .tint(.red)
                        }
                    }
                    .contextMenu {
                        if !known.online, !known.thisDevice {
                            Button("sync.devices.remove", systemImage: "trash", role: .destructive) {
                                features.forgetSyncDevices([known])
                            }
                        }
                    }
            }
            if others.isEmpty {
                Text("sync.devices.none")
                    .foregroundStyle(.secondary)
            }
            Button("sync.devices.add", systemImage: "plus.circle") {
                pairing = true
            }
        } header: {
            HStack {
                Text("sync.devices")
                Spacer()
                if away.count > 1 {
                    Menu {
                        Button(said("sync.devices.remove-away", away.count), systemImage: "trash", role: .destructive) {
                            features.forgetSyncDevices(away)
                        }
                    } label: {
                        Image(systemName: "ellipsis.circle")
                            .touchTarget()
                            .accessibilityLabel("sync.devices.options")
                    }
                }
            }
        } footer: {
            if !away.isEmpty {
                Text("sync.devices.footer")
            }
        }
    }
}

struct KnownDeviceEntry: View {
    let known: SyncedDevice

    var body: some View {
        if !known.thisDevice, known.online || known.nearby {
            NavigationLink(value: Destination.syncDevice(known.id)) {
                KnownDeviceRow(known: known)
            }
            .accessibilityHint("sync.devices.drive-hint")
        } else {
            KnownDeviceRow(known: known)
        }
    }
}

struct KnownDeviceRow: View {
    @Environment(\.accessibilityDifferentiateWithoutColor) private var differentiate
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
                    .fill(known.online ? Color.confirmation : Color.secondary.opacity(differentiate ? 0 : 0.35))
                    .overlay {
                        if differentiate, !known.online { Circle().strokeBorder(Color.secondary, lineWidth: 1.5) }
                    }
                    .frame(width: 8, height: 8)
                    .accessibilityHidden(true)
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var caption: String {
        let state = if known.thisDevice {
            said("sync.device.this-iphone")
        } else if known.nearby {
            said("sync.device.nearby")
        } else if known.online {
            said("sync.device.online")
        } else {
            said("sync.device.away")
        }
        return [known.platform.title, state].compactMap { $0 }.joined(separator: " · ")
    }
}

struct PairingSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var scenePhase
    @State private var joining = ""
    @State private var confirmingJoin = false
    @State private var confirmingNew = false
    @State private var copies = 0
    @State private var revealed = false

    private static let copiedFor: TimeInterval = 120

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
                                .frame(maxWidth: .infinity)
                                .blur(radius: revealed ? 0 : 9)
                                .privacySensitive()
                                .overlay {
                                    if !revealed {
                                        Label("sync.pairing.tap-to-show", systemImage: "eye")
                                            .font(.subheadline.weight(.semibold))
                                            .padding(.horizontal, 14)
                                            .padding(.vertical, 8)
                                            .glassEffect(.regular, in: .capsule)
                                    }
                                }
                                .contentShape(Rectangle())
                                .onTapGesture { withAnimation(reduceMotion ? nil : .smooth) { revealed.toggle() } }
                                .accessibilityLabel(revealed ? code : said("sync.pairing.hidden"))
                                .accessibilityHint(said(revealed ? "sync.pairing.hides" : "sync.pairing.shows"))
                                .accessibilityAddTraits(.isButton)
                        } else {
                            Text("sync.pairing.not-made")
                                .foregroundStyle(.secondary)
                                .frame(maxWidth: .infinity)
                        }
                        if let code {
                            HStack(spacing: 12) {
                                Button {
                                    UIPasteboard.general.setItems(
                                        [[UTType.plainText.identifier: code]],
                                        options: [
                                            .localOnly: true,
                                            .expirationDate: Date().addingTimeInterval(Self.copiedFor),
                                        ]
                                    )
                                    copies += 1
                                } label: {
                                    Label("common.copy", systemImage: "doc.on.doc")
                                }
                                ShareLink(item: code, subject: Text("sync.pairing.share-subject")) {
                                    Label("common.share", systemImage: "square.and.arrow.up")
                                }
                            }
                            .buttonStyle(.bordered)
                        }
                    }
                    .padding(.vertical, 8)
                } header: {
                    Text("sync.pairing.this-code")
                } footer: {
                    Text("sync.pairing.this-code.footer")
                }
                Section {
                    TextField("sync.pairing.code", text: $joining)
                        .font(.body.monospaced())
                        .textInputAutocapitalization(.characters)
                        .autocorrectionDisabled()
                    if !joining.isEmpty, !reads {
                        Text("sync.pairing.not-a-code")
                            .font(.caption)
                            .foregroundStyle(Color.failure)
                            .announced(said("sync.pairing.not-a-code"))
                    }
                    Button("sync.pairing.join") {
                        if code == nil { join() } else { confirmingJoin = true }
                    }
                    .disabled(!reads)
                } header: {
                    Text("sync.pairing.join.header")
                } footer: {
                    Text("sync.pairing.join.footer")
                }
                Section {
                    Button("sync.pairing.new", role: .destructive) {
                        confirmingNew = true
                    }
                } footer: {
                    Text("sync.pairing.new.footer")
                }
            }
            .navigationTitle("sync.pairing.title")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("common.done") { dismiss() }
                }
            }
            .sensoryFeedback(.success, trigger: copies)
            .onChange(of: scenePhase) { _, phase in
                if phase != .active { revealed = false }
            }
            .confirmationDialog("sync.pairing.join.confirm", isPresented: $confirmingJoin, titleVisibility: .visible) {
                Button("sync.pairing.join") { join() }
            } message: {
                Text("sync.pairing.join.confirm.message")
            }
            .confirmationDialog("sync.pairing.new.confirm", isPresented: $confirmingNew, titleVisibility: .visible) {
                Button("sync.pairing.new.confirm.action", role: .destructive) { library.features.newSyncGroup() }
            } message: {
                Text("sync.pairing.new.confirm.message")
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
                SyncSwitch(title: said("sync.songs.favourites"), detail: said("sync.songs.favourites.detail"), choice: \.favourites)
                SyncSwitch(title: said("sync.songs.kept-playlists"), detail: said("sync.songs.kept-playlists.detail"), choice: \.playlists)
                SyncSwitch(title: said("sync.songs.everything"), detail: said("sync.songs.everything.detail"), choice: \.everything)
            } header: {
                Text("sync.songs.fetch-automatically")
            } footer: {
                Text("sync.songs.fetch-automatically.footer")
            }
            Section {
                SyncSwitch(title: said("sync.songs.mobile-data"), detail: said("sync.songs.mobile-data.detail"), choice: \.meteredAllowed)
            }
            Section {
                if let first = destinations.first {
                    let kept = destinations.first { $0.id == features.syncFolder } ?? first
                    Picker("sync.songs.folder", selection: Binding(
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
                        "sync.songs.no-folder",
                        systemImage: "folder.badge.questionmark",
                        description: Text("sync.songs.no-folder.detail")
                    )
                }
                if let notice = features.syncNotice {
                    Label(notice, systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(Color.warning)
                        .announced(notice)
                }
            } footer: {
                Text("sync.songs.folder.footer")
            }
        }
        .navigationTitle("sync.options.songs")
        .task { library.folderList = await library.folders.refreshed() }
    }
}

struct SyncConnectionPage: View {
    @Environment(Library.self) private var library
    @State private var chosen: SyncRoute?
    @State private var server = ""
    @State private var device = ""
    @State private var sharedName = ""

    var body: some View {
        let features = library.features
        ThemedForm {
            if let config = features.syncConfig {
                let route = chosen ?? SyncRoute(config)
                Section {
                    Picker("sync.route", selection: Binding(get: { route }, set: { choose($0, config) })) {
                        Label("sync.route.hosted", systemImage: "cloud").tag(SyncRoute.hosted)
                        Label("sync.route.own", systemImage: "server.rack").tag(SyncRoute.own)
                        Label("sync.route.nearby", systemImage: "wifi").tag(SyncRoute.nearby)
                    }
                    .pickerStyle(.inline)
                    .labelsHidden()
                } header: {
                    Text("sync.route")
                } footer: {
                    Text(Self.explanation(route))
                }
                if route == .own {
                    Section {
                        TextField("sync.route.own.address-prompt", text: $server)
                            .keyboardType(.URL)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                        Button("sync.route.own.use") {
                            guard var next = features.syncConfig else { return }
                            next.server = trimmed(server)
                            next.nearbyOnly = false
                            features.saveSync(next)
                        }
                        .disabled(trimmed(server).isEmpty || (trimmed(server) == config.server && !config.nearbyOnly))
                    } header: {
                        Text("sync.route.own.address")
                    }
                }
                Section {
                    TextField("common.name", text: $device, prompt: Text("iPhone"))
                    Button("sync.connection.save-name") {
                        guard var next = features.syncConfig else { return }
                        next.device = trimmed(device)
                        features.saveSync(next)
                    }
                    .disabled(trimmed(device) == config.device)
                } header: {
                    Text("sync.connection.device-name")
                } footer: {
                    Text("sync.connection.device-name.footer")
                }
                Section {
                    TextField("common.name", text: $sharedName)
                    Button("sync.connection.save-name") { features.together.rename(sharedName) }
                        .disabled(trimmed(sharedName) == features.together.name)
                } header: {
                    Text("sync.connection.shared-name")
                } footer: {
                    Text("sync.connection.shared-name.footer")
                }
            }
        }
        .navigationTitle("sync.options.connection")
        .onAppear {
            sharedName = features.together.name
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
            said("sync.route.hosted.explained")
        case .own:
            said("sync.route.own.explained")
        case .nearby:
            said("sync.route.nearby.explained")
        }
    }
}

struct SyncTransfers: View {
    let transfers: [SyncTransfer]
    let downloading: UInt32
    let waiting: UInt32
    private static let finishedShown = 10

    var body: some View {
        let active = transfers.filter { $0.state == .waiting || $0.state == .moving }
        let finished = transfers.filter { $0.state == .landed || $0.state == .failed }.suffix(Self.finishedShown).reversed()
        Section {
            ForEach(SyncTransferKeys.keyed(active, as: "active"), id: \.key) { keyed in
                SyncTransferRow(transfer: keyed.transfer)
            }
            ForEach(SyncTransferKeys.keyed(Array(finished), as: "finished"), id: \.key) { keyed in
                SyncTransferRow(transfer: keyed.transfer)
            }
        } header: {
            Text(active.isEmpty ? said("sync.transfers.recent") : fetchingHeading(downloading: downloading, waiting: waiting))
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
                    Text(transfer.title.isEmpty ? said("sync.transfer.unknown-song") : transfer.title).titleLines()
                    Text(caption)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .titleLines()
                        .monospacedDigit()
                }
            }
            if transfer.state == .moving, transfer.size > 0 {
                ProgressView(value: Double(min(transfer.received, transfer.size)), total: Double(transfer.size))
                    .accessibilityLabel("sync.transfer.fetching")
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
        case .landed: Color.confirmation
        case .failed: Color.warning
        }
    }

    private var caption: String {
        let source = [transfer.artist, transfer.from.isEmpty ? nil : said("sync.transfer.from", transfer.from)]
            .compactMap { $0?.isEmpty == false ? $0 : nil }
            .joined(separator: " · ")
        let state: String = switch transfer.state {
        case .waiting: said("sync.transfer.waiting")
        case .moving where transfer.size > 0:
            said(
                "sync.transfer.progress",
                Int64(transfer.received).formatted(.byteCount(style: .file)),
                Int64(transfer.size).formatted(.byteCount(style: .file))
            )
        case .moving: said("sync.transfer.fetching")
        case .landed: said("sync.transfer.fetched")
        case .failed: said("sync.transfer.failed")
        }
        return source.isEmpty ? state : "\(state) · \(source)"
    }
}

extension DevicePlatform {
    var title: String? {
        switch self {
        case .linux: said("platform.linux")
        case .android: said("platform.android")
        case .ios: said("platform.ios")
        case .windows: said("platform.windows")
        case .macOs: said("platform.macos")
        case .other: nil
        }
    }

    var symbol: String {
        switch self {
        case .linux, .windows, .macOs: "desktopcomputer"
        case .android, .ios: "iphone"
        case .other: "questionmark.square.dashed"
        }
    }
}

enum SyncControlsOffered {
    static let shown = syncControlOffered()
}
