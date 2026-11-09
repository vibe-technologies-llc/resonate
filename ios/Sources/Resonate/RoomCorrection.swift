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
    private static let yawKey = "head-yaw"
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
    private(set) var yaw: Int
    @ObservationIgnored private var changing: Task<Void, Never>?

    init(library: Library) {
        self.library = library
        on = settings.bool(forKey: Self.onKey)
        name = settings.string(forKey: Self.nameKey)
        angle = settings.object(forKey: Self.angleKey) as? Int ?? Int(Self.limits.angleStandardDegrees)
        elevation = settings.object(forKey: Self.elevationKey) as? Int ?? 0
        yaw = settings.object(forKey: Self.yawKey) as? Int ?? 0
    }

    func restore(_ native: MobilePlayer) {
        let on = on
        let heldAngle = UInt8(clamping: angle)
        let heldElevation = Int16(clamping: elevation)
        let heldYaw = Int16(clamping: yaw)
        queue { [self] in
            do {
                _ = try await Work.aside { try native.setSpeakerAngle(degrees: heldAngle) }
                _ = try await Work.aside { try native.setSpeakerElevation(degrees: heldElevation) }
                _ = try await Work.aside { try native.setHeadYaw(degrees: heldYaw) }
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
                let kept = try await Work.aside {
                    try Self.staged(url, as: extensionName, by: native)
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

    func turn(to degrees: Int) {
        queue { [self] in
            do {
                let native = try await library.player()
                let moved = try await Work.aside { try native.setHeadYaw(degrees: Int16(clamping: degrees)) }
                kept = moved ?? kept
                yaw = degrees
                settings.set(degrees, forKey: Self.yawKey)
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
        library.notice(said("room.gone"))
    }

    private func remember(on: Bool) {
        self.on = on
        settings.set(on, forKey: Self.onKey)
    }

    nonisolated private static func staged(
        _ url: URL,
        as extensionName: String,
        by native: MobilePlayer
    ) throws -> RoomCorrection {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        try native.beginRoomCorrection(extension: extensionName)
        do {
            let chunkBytes = Int(roomCorrectionChunkBytes())
            while let chunk = try handle.read(upToCount: chunkBytes), !chunk.isEmpty {
                try native.stageRoomCorrection(chunk: chunk)
            }
            return try native.finishRoomCorrection()
        } catch {
            try? native.abandonRoomCorrection()
            throw error
        }
    }

    nonisolated private static func contents(of url: URL, atMost largest: Int) throws -> Data {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        return try handle.read(upToCount: largest + 1) ?? Data()
    }
}

struct RoomCorrectionView: View {
    private static let responseTypes: [UTType] =
        [.audio] + (UTType(filenameExtension: "sofa").map { [$0] } ?? [])

    @Environment(Library.self) private var library
    @State private var choosing = false
    @State private var forgetting = false
    @State private var choosingLeft = false
    @State private var choosingRight = false

    var body: some View {
        let room = library.features.room
        let chosen = room.name != nil
        ThemedForm {
            Section {
                if let name = room.name {
                    Toggle(isOn: Binding(get: { room.on }, set: { room.turn($0) })) {
                        ToolRow(title: said("room.correct"), detail: room.kept.map(Self.described).map { "\(name) · \($0)" } ?? name, symbol: "waveform.path")
                    }
                    .disabled(room.reading)
                }
                if let left = room.left {
                    LabeledContent("room.left-speaker", value: left.lastPathComponent)
                    Button { choosingRight = true } label: {
                        ToolRow(
                            title: said(room.reading ? "room.reading" : "room.choose-right"),
                            detail: said("room.choose-right.detail"),
                            symbol: "waveform.path"
                        )
                    }
                    .disabled(room.reading)
                    .fileImporter(isPresented: $choosingRight, allowedContentTypes: Self.responseTypes) { result in
                        switch result {
                        case .success(let url): room.chooseRight(url)
                        case .failure(let failure): library.report(failure)
                        }
                    }
                    Button("common.cancel", systemImage: "xmark") { room.cancelPair() }
                        .disabled(room.reading)
                } else {
                    Button { choosing = true } label: {
                        ToolRow(
                            title: said(room.reading ? "room.reading" : chosen ? "room.choose-another" : "room.choose"),
                            detail: said("room.choose.detail"),
                            symbol: "doc.badge.plus"
                        )
                    }
                    .disabled(room.reading)
                    .fileImporter(isPresented: $choosing, allowedContentTypes: Self.responseTypes) { result in
                        switch result {
                        case .success(let url): room.choose(url)
                        case .failure(let failure): library.report(failure)
                        }
                    }
                    Button { choosingLeft = true } label: {
                        ToolRow(title: said("room.choose-pair"), detail: said("room.choose-pair.detail"), symbol: "hifispeaker.2")
                    }
                    .disabled(room.reading)
                    .fileImporter(isPresented: $choosingLeft, allowedContentTypes: Self.responseTypes) { result in
                        switch result {
                        case .success(let url): room.chooseLeft(url)
                        case .failure(let failure): library.report(failure)
                        }
                    }
                }
                if chosen {
                    Button(role: .destructive) { forgetting = true } label: {
                        ToolRow(title: said("room.forget-response"), detail: said("room.forget-response.detail"), symbol: "trash", ink: Color.failure)
                    }
                    .disabled(room.reading)
                }
            } header: {
                Text("room.response")
            } footer: {
                Text("room.response.footer")
            }
            if chosen {
                Section {
                    Stepper(
                        value: Binding(get: { room.angle }, set: { room.speak(at: $0) }),
                        in: Int(RoomCorrectionState.limits.angleLeastDegrees)...Int(RoomCorrectionState.limits.angleMostDegrees)
                    ) {
                        LabeledContent("room.speaker-angle", value: "\(room.angle)°")
                    }
                    .accessibilityIdentifier("speaker-angle")
                    Stepper(
                        value: Binding(get: { room.elevation }, set: { room.raise(to: $0) }),
                        in: Int(RoomCorrectionState.limits.elevationLeastDegrees)...Int(RoomCorrectionState.limits.elevationMostDegrees)
                    ) {
                        LabeledContent("room.speaker-elevation", value: "\(room.elevation)°")
                    }
                    .accessibilityIdentifier("speaker-elevation")
                    Stepper(
                        value: Binding(get: { room.yaw }, set: { room.turn(to: $0) }),
                        in: Int(RoomCorrectionState.limits.yawLeastDegrees)...Int(RoomCorrectionState.limits.yawMostDegrees)
                    ) {
                        LabeledContent("room.head-turned", value: "\(room.yaw)°")
                    }
                    .accessibilityIdentifier("head-yaw")
                } header: {
                    Text("room.speakers")
                } footer: {
                    Text("room.speakers.footer")
                }
            }
        }
        .navigationTitle("room.title")
        .confirmationDialog("room.forget.question", isPresented: $forgetting, titleVisibility: .visible) {
            Button("room.forget", role: .destructive) { room.forget() }
        }
    }

    private static func described(_ kept: RoomCorrection) -> String {
        let channels =
            switch kept.channels {
            case 1: said("room.channels.mono")
            case 2: said("room.channels.stereo")
            case 4: said("room.channels.binaural")
            default: said("room.channels", Int(kept.channels))
            }
        let rate = said("room.rate", String(format: "%g", Double(kept.rate) / 1000))
        let length = said("room.length", String(format: "%.2f", Double(kept.lengthMs) / 1000))
        return "\(channels) · \(rate) · \(length)"
    }
}
