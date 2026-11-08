import ResonateLogic
import ResonateNative
import SwiftUI

struct DrivenDeviceView: View {
    @Environment(Library.self) private var library
    let device: SyncedDevice
    @State private var shown: RemoteDevice?

    private static let readEvery: Duration = .milliseconds(500)
    private static let volumeStep: UInt16 = 100
    private static let wholeVolume: UInt16 = 1000

    var body: some View {
        List {
            switch shown?.standing {
            case nil, .waiting:
                Text("Asking \(device.name) what it plays…")
                    .foregroundStyle(.secondary)
            case .refused:
                Text("\(device.name) does not let others drive it, or cannot be driven.")
                    .foregroundStyle(.secondary)
            case .mirrored:
                ForEach(shown?.zones ?? [], id: \.zone) { zone in
                    Section(zone.name) {
                        zoneRows(zone)
                    }
                    if !zone.rows.isEmpty {
                        Section("Queue of \(zone.queueLength)") {
                            queueRows(zone)
                        }
                    }
                }
            }
        }
        .navigationTitle(device.name)
        .task(id: device.id) {
            await follow()
        }
    }

    @ViewBuilder
    private func zoneRows(_ zone: RemoteZoneState) -> some View {
        let sounding = zone.playing == .playing
        VStack(alignment: .leading, spacing: 4) {
            Text(Self.song(zone))
                .font(.headline)
                .lineLimit(2)
            Text("\(Self.place(zone)) · \((Int(zone.volumePermille) + 5) / 10)%")
                .font(.caption.monospacedDigit())
                .foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .combine)
        if let length = zone.lengthMillis, length > 0 {
            DrivenSeek(zone: zone.zone, positionMillis: zone.positionMillis, lengthMillis: length) { control in
                press(zone.zone, control)
            }
        }
        HStack(spacing: 24) {
            control("Previous song", "backward.fill", zone, .previous)
            control(sounding ? "Pause" : "Play", sounding ? "pause.fill" : "play.fill", zone, .toggle)
            control("Next song", "forward.fill", zone, .next)
            control("Quieter", "speaker.minus.fill", zone, .volume(permille: zone.volumePermille > Self.volumeStep ? zone.volumePermille - Self.volumeStep : 0))
            control("Louder", "speaker.plus.fill", zone, .volume(permille: min(Self.wholeVolume, zone.volumePermille + Self.volumeStep)))
        }
        .buttonStyle(.borderless)
        .frame(maxWidth: .infinity)
    }

    @ViewBuilder
    private func queueRows(_ zone: RemoteZoneState) -> some View {
        ForEach(Array(zone.rows.enumerated()), id: \.offset) { offset, song in
            let row = DrivenRows.row(offset, firstRow: zone.firstRow)
            let up = DrivenRows.moveUp(row)
            let down = DrivenRows.moveDown(row, queueLength: zone.queueLength)
            Button {
                press(zone.zone, .jump(row: row))
            } label: {
                DrivenQueueRow(song: song, playing: zone.playingRow == row)
            }
            .buttonStyle(.plain)
            .swipeActions(edge: .trailing) {
                Button("Remove from queue", systemImage: "minus.circle", role: .destructive) {
                    press(zone.zone, .remove(row: row))
                }
                .tint(.red)
            }
            .contextMenu {
                if let up {
                    Button("Move up", systemImage: "arrow.up") { press(zone.zone, up) }
                }
                if let down {
                    Button("Move down", systemImage: "arrow.down") { press(zone.zone, down) }
                }
                Button("Remove from queue", systemImage: "minus.circle", role: .destructive) {
                    press(zone.zone, .remove(row: row))
                }
            }
            .accessibilityAction(named: "Remove from queue") { press(zone.zone, .remove(row: row)) }
        }
    }

    private func control(_ title: String, _ symbol: String, _ zone: RemoteZoneState, _ remote: RemoteControl) -> some View {
        Button(title, systemImage: symbol) {
            press(zone.zone, remote)
        }
        .labelStyle(.iconOnly)
        .font(.title2)
    }

    private func follow() async {
        defer {
            let id = device.id
            Task {
                guard let native = try? await library.player() else { return }
                try? await Work.aside { native.syncUnwatch(device: id) }
            }
        }
        while !Task.isCancelled {
            if let native = try? await library.player() {
                let id = device.id
                if let read = try? await Work.aside({ try native.syncWatch(device: id) }), read != shown {
                    shown = read
                }
            }
            try? await Task.sleep(for: Self.readEvery)
        }
    }

    private func press(_ zone: UInt32, _ remote: RemoteControl) {
        let id = device.id
        Task {
            do {
                let native = try await library.player()
                let answer = try await Work.aside { try native.syncControl(device: id, zone: zone, control: remote) }
                if let refused = Self.refusal(answer) { library.notice(refused) }
            } catch {
                library.report(error)
            }
        }
    }

    static func song(_ zone: RemoteZoneState) -> String {
        guard let playing = zone.playingRow, playing >= zone.firstRow else { return "Nothing playing" }
        let index = Int(playing - zone.firstRow)
        guard index < zone.rows.count else { return "Nothing playing" }
        return billed(zone.rows[index])
    }

    static func billed(_ row: RemoteSong) -> String {
        row.artist.isEmpty ? row.title : "\(row.title) — \(row.artist)"
    }

    static func place(_ zone: RemoteZoneState) -> String {
        let at = clock(zone.positionMillis)
        guard let length = zone.lengthMillis else { return at }
        return "\(at) / \(clock(length))"
    }

    static func clock(_ millis: UInt64) -> String {
        let seconds = millis / 1000
        let (hours, minutes, rest) = (seconds / 3600, seconds / 60 % 60, seconds % 60)
        let two = { (figure: UInt64) in figure < 10 ? "0\(figure)" : "\(figure)" }
        return hours > 0 ? "\(hours):\(two(minutes)):\(two(rest))" : "\(minutes):\(two(rest))"
    }

    static func refusal(_ answer: RemoteAnswer) -> String? {
        switch answer {
        case .done: nil
        case .untrusted: "That device does not let others do that"
        case .uncontrollable: "That device plays nothing another can drive"
        case .noSuchZone: "That zone is gone"
        case .noSuchRow: "That song is no longer in its queue"
        case .failed: "The other device could not do that"
        case .unanswered: "The other device did not answer"
        }
    }
}

private struct DrivenQueueRow: View {
    let song: RemoteSong
    let playing: Bool

    var body: some View {
        HStack {
            Text(DrivenDeviceView.billed(song))
                .fontWeight(playing ? .semibold : .regular)
                .foregroundStyle(playing ? Color.worn : Color.primary)
                .lineLimit(2)
            Spacer(minLength: 8)
            if let length = song.lengthMillis {
                Text(DrivenDeviceView.clock(length))
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        }
        .contentShape(.rect)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(playing ? .isSelected : [])
    }
}

private struct DrivenSeek: View {
    let zone: UInt32
    let positionMillis: UInt64
    let lengthMillis: UInt64
    let commit: (RemoteControl) -> Void
    @State private var editing: Double?

    var body: some View {
        CommittingSlider(
            title: "Position",
            value: min(Double(positionMillis), Double(lengthMillis)) / 1000,
            range: 0...(Double(lengthMillis) / 1000),
            step: 1,
            editing: $editing,
            spoken: { DrivenDeviceView.clock(UInt64($0 * 1000)) },
            commit: { commit(DrivenRows.seek(to: $0, lengthMillis: lengthMillis)) }
        )
    }
}

struct SyncControlPage: View {
    @Environment(Library.self) private var library

    var body: some View {
        let offered = SyncControlsOffered.shown
        let features = library.features
        ThemedForm {
            if let config = features.syncConfig {
                Section {
                    Picker(offered.asked, selection: Binding(
                        get: { config.control },
                        set: { chosen in
                            guard var next = library.features.syncConfig else { return }
                            next.control = chosen
                            library.features.saveSync(next)
                        }
                    )) {
                        ForEach(offered.choices, id: \.label) { choice in
                            VStack(alignment: .leading, spacing: 2) {
                                Text(choice.label)
                                Text(choice.meaning).font(.caption).foregroundStyle(.secondary)
                            }
                            .tag(choice.control)
                        }
                    }
                    .pickerStyle(.inline)
                    .labelsHidden()
                } header: {
                    Text(offered.asked)
                } footer: {
                    Text(offered.told)
                }
            }
        }
        .navigationTitle(offered.title)
    }
}
