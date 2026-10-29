import Foundation
import Observation
import ResonateLogic
import ResonateNative
import SwiftUI
import UniformTypeIdentifiers

@MainActor
@Observable
final class RoomCorrectionState {
    private static let onKey = "room-correction"
    private static let nameKey = "room-correction-name"
    private static let angleKey = "speaker-angle"
    private static let elevationKey = "speaker-elevation"
    static let limits = speakerLimits()
    nonisolated private static let largest = 64 * 1024 * 1024

    @ObservationIgnored private unowned let library: Library
    @ObservationIgnored private let settings = UserDefaults(suiteName: "playback") ?? .standard

    private(set) var on: Bool
    private(set) var name: String?
    private(set) var kept: RoomCorrection?
    private(set) var reading = false
    private(set) var left: URL?
    private(set) var angle: Int
    private(set) var elevation: Int
    @ObservationIgnored private var changing: Task<Void, Never>?

    init(library: Library) {
        self.library = library
        on = settings.bool(forKey: Self.onKey)
        name = settings.string(forKey: Self.nameKey)
        angle = settings.object(forKey: Self.angleKey) as? Int ?? Int(Self.limits.angleStandardDegrees)
        elevation = settings.object(forKey: Self.elevationKey) as? Int ?? 0
    }

    func restore(_ native: MobilePlayer) {
        let on = on
        let heldAngle = UInt8(clamping: angle)
        let heldElevation = Int16(clamping: elevation)
        queue { [self] in
            do {
                _ = try await Work.aside { try native.setSpeakerAngle(degrees: heldAngle) }
                _ = try await Work.aside { try native.setSpeakerElevation(degrees: heldElevation) }
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

    func speak(at degrees: Int) {
        queue { [self] in
            do {
                let native = try await library.player()
                let moved = try await Work.aside { try native.setSpeakerAngle(degrees: UInt8(clamping: degrees)) }
                kept = moved ?? kept
                angle = degrees
                settings.set(degrees, forKey: Self.angleKey)
            } catch {
                library.report(error)
            }
        }
    }

    func raise(to degrees: Int) {
        queue { [self] in
            do {
                let native = try await library.player()
                let moved = try await Work.aside { try native.setSpeakerElevation(degrees: Int16(clamping: degrees)) }
                kept = moved ?? kept
                elevation = degrees
                settings.set(degrees, forKey: Self.elevationKey)
            } catch {
                library.report(error)
            }
        }
    }

    func chooseLeft(_ url: URL) {
        left = url
    }

    func cancelPair() {
        left = nil
    }

    func chooseRight(_ url: URL) {
        guard let left else { return }
        let named = RoomPair.name(left: left.lastPathComponent, right: url.lastPathComponent)
        let leftExtension = RoomPair.fileExtension(of: left.lastPathComponent)
        let rightExtension = RoomPair.fileExtension(of: url.lastPathComponent)
        reading = true
        queue { [self] in
            defer {
                reading = false
                self.left = nil
            }
            do {
                let native = try await library.player()
                let largest = Self.largest
                let leftBytes = try await Work.aside { try Self.contents(of: left, atMost: largest) }
                let rightBytes = try await Work.aside { try Self.contents(of: url, atMost: largest) }
                let kept = try await Work.aside {
                    try native.keepBinauralRoomCorrection(
                        leftBytes: leftBytes,
                        leftExtension: leftExtension,
                        rightBytes: rightBytes,
                        rightExtension: rightExtension
                    )
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
    private static let responseTypes: [UTType] =
        [.audio] + (UTType(filenameExtension: "sofa").map { [$0] } ?? [])

    @Environment(Library.self) private var library
    @State private var choosing = false
    @State private var forgetting = false
    @State private var choosingLeft = false
    @State private var choosingRight = false

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
                .disabled(room.reading || room.left != nil)
                .fileImporter(isPresented: $choosing, allowedContentTypes: Self.responseTypes) { result in
                    switch result {
                    case .success(let url): room.choose(url)
                    case .failure(let failure): library.report(failure)
                    }
                }
            if let left = room.left {
                LabeledContent("Left speaker", value: left.lastPathComponent)
                Button(room.reading ? "Reading…" : "Choose the right speaker's response…", systemImage: "waveform.path") {
                    choosingRight = true
                }
                .disabled(room.reading)
                .fileImporter(isPresented: $choosingRight, allowedContentTypes: Self.responseTypes) { result in
                    switch result {
                    case .success(let url): room.chooseRight(url)
                    case .failure(let failure): library.report(failure)
                    }
                }
                Button("Cancel", systemImage: "xmark") { room.cancelPair() }
                    .disabled(room.reading)
            } else {
                Button("Choose a pair…", systemImage: "headphones") { choosingLeft = true }
                    .disabled(room.reading)
                    .fileImporter(isPresented: $choosingLeft, allowedContentTypes: Self.responseTypes) { result in
                        switch result {
                        case .success(let url): room.chooseLeft(url)
                        case .failure(let failure): library.report(failure)
                        }
                    }
            }
            Stepper(
                value: Binding(get: { room.angle }, set: { room.speak(at: $0) }),
                in: Int(RoomCorrectionState.limits.angleLeastDegrees)...Int(RoomCorrectionState.limits.angleMostDegrees)
            ) {
                LabeledContent("Speaker angle", value: "\(room.angle)°")
            }
            .accessibilityIdentifier("speaker-angle")
            Stepper(
                value: Binding(get: { room.elevation }, set: { room.raise(to: $0) }),
                in: Int(RoomCorrectionState.limits.elevationLeastDegrees)...Int(RoomCorrectionState.limits.elevationMostDegrees)
            ) {
                LabeledContent("Speaker elevation", value: "\(room.elevation)°")
            }
            .accessibilityIdentifier("speaker-elevation")
            if room.name != nil {
                Button("Forget the response", systemImage: "trash", role: .destructive) { forgetting = true }
            }
        } header: {
            Text("Room correction")
        } footer: {
            Text("A measured impulse response, up to ten seconds long, is convolved before the equaliser on every output. It is lowered where it would lift the music past full scale. A four-channel file is a two-by-two (binaural) response, in the order left-to-left, left-to-right, right-to-left, right-to-right. A SOFA set of head-related responses is read as the same, from the measurements around the speaker angle either side of the front and the elevation above the ears, blended where several surround them. A pair is two stereo responses, the left speaker's and then the right's, each as heard at both ears, joined into one two-by-two response.")
        }
        .confirmationDialog("Forget the room correction?", isPresented: $forgetting, titleVisibility: .visible) {
            Button("Forget", role: .destructive) { room.forget() }
        }
    }

    private static func described(_ kept: RoomCorrection) -> String {
        let channels = kept.channels == 1 ? "Mono" : kept.channels == 2 ? "Stereo" : kept.channels == 4 ? "Two-by-two (binaural)" : "\(kept.channels) channels"
        let rate = String(format: "%g kHz", Double(kept.rate) / 1000)
        let length = String(format: "%.2f s", Double(kept.lengthMs) / 1000)
        return "\(channels) · \(rate) · \(length)"
    }
}
