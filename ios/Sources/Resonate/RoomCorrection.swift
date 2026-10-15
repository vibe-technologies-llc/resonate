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

    init(library: Library) {
        self.library = library
        on = settings.bool(forKey: Self.onKey)
        name = settings.string(forKey: Self.nameKey)
    }

    func restore(_ native: MobilePlayer) {
        let on = on
        Task {
            do {
                kept = try await Work.aside { try native.applyRoomCorrection(on: on) }
                if kept == nil, on { remember(on: false) }
            } catch {
                remember(on: false)
                library.report(error)
            }
        }
    }

    func turn(_ wanted: Bool) {
        library.request({ native in try native.applyRoomCorrection(on: wanted) }) { [weak self] applied in
            self?.kept = applied ?? self?.kept
            self?.remember(on: wanted && applied != nil)
        }
    }

    func choose(_ url: URL) {
        let named = url.deletingPathExtension().lastPathComponent
        let extensionName = url.pathExtension
        reading = true
        Task {
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
                turn(true)
            } catch {
                library.report(error)
            }
        }
    }

    func forget() {
        library.request({ native in try native.forgetRoomCorrection() }) { [weak self] in
            self?.kept = nil
            self?.name = nil
            self?.settings.removeObject(forKey: Self.nameKey)
            self?.remember(on: false)
        }
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
