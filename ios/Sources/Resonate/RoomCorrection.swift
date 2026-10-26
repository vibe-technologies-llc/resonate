import Foundation
import Observation
import ResonateNative
import SwiftUI
import UniformTypeIdentifiers

@MainActor
@Observable
final class RoomCorrectionState {
    private static let onKey = "room-correction"
    private static let nameKey = "room-correction-name"
    nonisolated private static let largest = 64 * 1024 * 1024

    @ObservationIgnored private unowned let library: Library
    @ObservationIgnored private let settings = UserDefaults(suiteName: "playback") ?? .standard

    private(set) var on: Bool
    private(set) var name: String?
    private(set) var kept: RoomCorrection?
    private(set) var reading = false
    @ObservationIgnored private var changing: Task<Void, Never>?

    init(library: Library) {
        self.library = library
        on = settings.bool(forKey: Self.onKey)
        name = settings.string(forKey: Self.nameKey)
    }

    func restore(_ native: MobilePlayer) {
        let on = on
        queue { [self] in
            do {
                kept = try await Work.aside { try native.keptRoomCorrection() }
                if on {
                    let applied = try await Work.aside { try native.applyRoomCorrection(on: true) }
                    if applied == nil { gone() }
                }
            } catch {
                library.report(error)
            }
        }
    }

    func turn(_ wanted: Bool) {
        queue { [self] in await apply(wanted) }
    }

    func choose(_ url: URL) {
        let named = url.deletingPathExtension().lastPathComponent
        let extensionName = url.pathExtension
        reading = true
        queue { [self] in
            defer { reading = false }
            do {
                let native = try await library.player()
                let largest = Self.largest
                let bytes = try await Work.aside { try Self.contents(of: url, atMost: largest) }
                let kept = try await Work.aside {
                    try native.keepRoomCorrection(bytes: bytes, extension: extensionName)
                }
                self.kept = kept
                name = named
                settings.set(named, forKey: Self.nameKey)
                await apply(true)
            } catch {
                library.report(error)
            }
        }
    }

    func forget() {
        queue { [self] in
            do {
                let native = try await library.player()
                try await Work.aside { try native.forgetRoomCorrection() }
                kept = nil
                name = nil
                settings.removeObject(forKey: Self.nameKey)
                remember(on: false)
            } catch {
                library.report(error)
            }
        }
    }

    private func queue(_ step: @escaping @MainActor () async -> Void) {
        let previous = changing
        changing = Task {
            await previous?.value
            await step()
        }
    }

    private func apply(_ wanted: Bool) async {
        do {
            let native = try await library.player()
            let applied = try await Work.aside { try native.applyRoomCorrection(on: wanted) }
            kept = applied ?? kept
            remember(on: wanted && applied != nil)
            if wanted, applied == nil { gone() }
        } catch {
            library.report(error)
        }
    }

    private func gone() {
        remember(on: false)
        library.notice("The room response is no longer kept, so room correction stays off")
    }

    private func remember(on: Bool) {
        self.on = on
        settings.set(on, forKey: Self.onKey)
    }

    nonisolated private static func contents(of url: URL, atMost largest: Int) throws -> Data {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        return try handle.read(upToCount: largest + 1) ?? Data()
    }
}

struct RoomCorrectionSection: View {
    @Environment(Library.self) private var library
    @State private var choosing = false
    @State private var forgetting = false

    var body: some View {
        let room = library.features.room
        Section {
            Toggle("Correct for the room", isOn: Binding(get: { room.on }, set: { room.turn($0) }))
                .disabled(room.kept == nil && room.name == nil)
            if let name = room.name {
                LabeledContent(name, value: room.kept.map(Self.described) ?? "")
            }
            Button(room.reading ? "Reading…" : room.name == nil ? "Choose a response…" : "Choose another response…",
                   systemImage: "waveform.path") { choosing = true }
                .disabled(room.reading)
            if room.name != nil {
                Button("Forget the response", systemImage: "trash", role: .destructive) { forgetting = true }
            }
        } header: {
            Text("Room correction")
        } footer: {
            Text("A measured impulse response, up to ten seconds long, is convolved before the equaliser on every output. It is lowered where it would lift the music past full scale.")
        }
        .fileImporter(isPresented: $choosing, allowedContentTypes: [.audio]) { result in
            switch result {
            case .success(let url): room.choose(url)
            case .failure(let failure): library.report(failure)
            }
        }
        .confirmationDialog("Forget the room correction?", isPresented: $forgetting, titleVisibility: .visible) {
            Button("Forget", role: .destructive) { room.forget() }
        }
    }

    private static func described(_ kept: RoomCorrection) -> String {
        let channels = kept.channels == 1 ? "Mono" : kept.channels == 2 ? "Stereo" : "\(kept.channels) channels"
        let rate = String(format: "%g kHz", Double(kept.rate) / 1000)
        let length = String(format: "%.2f s", Double(kept.lengthMs) / 1000)
        return "\(channels) · \(rate) · \(length)"
    }
}
