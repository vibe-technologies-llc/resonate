import Foundation
import Observation
import ResonateLogic
import ResonateNative
import SwiftUI

@MainActor
@Observable
final class Enrichment {
    private static let pollEvery: Duration = .seconds(2)
    private static let automaticKey = "enrich-automatically"
    private static let unmeteredKey = "enrich-unmetered"
    private static let everything = EnrichmentOptions(refresh: false, atMost: nil)
    private static let refreshing = EnrichmentOptions(refresh: true, atMost: nil)

    var status: EnrichmentStatus?
    var coversAskedAgain: UInt32?
    var automatic: Bool {
        didSet {
            settings.set(automatic, forKey: Self.automaticKey)
            guard automatic != oldValue else { return }
            if automatic { begin() } else if running { stop() }
        }
    }
    var unmeteredOnly: Bool {
        didSet {
            settings.set(unmeteredOnly, forKey: Self.unmeteredKey)
            guard unmeteredOnly != oldValue else { return }
            if !unmeteredOnly {
                begin()
            } else if running, !library.features.unmetered {
                stop()
            }
        }
    }

    @ObservationIgnored private unowned let library: Library
    @ObservationIgnored private let settings = UserDefaults(suiteName: "library-features") ?? .standard
    @ObservationIgnored private var watching: Task<Void, Never>?
    @ObservationIgnored private var foreground = true
    @ObservationIgnored private var erased = false
    @ObservationIgnored private var sought: UInt64?
    @ObservationIgnored private var portraits: UInt32 = 0
    @ObservationIgnored private var unsettled = false

    init(library: Library) {
        self.library = library
        automatic = settings.object(forKey: Self.automaticKey) == nil ? true : settings.bool(forKey: Self.automaticKey)
        unmeteredOnly = settings.bool(forKey: Self.unmeteredKey)
    }

    var running: Bool { status?.running == true }

    var line: String { Self.line(of: status) }

    private var mayAsk: Bool {
        let features = library.features
        return automatic && features.online && (!unmeteredOnly || features.unmetered)
    }

    func begin() {
        guard mayAsk else { return }
        start(Self.everything, stoppingFirst: false, seeking: library.snapshot?.current?.id)
    }

    func refresh() {
        start(Self.refreshing, stoppingFirst: true, seeking: library.snapshot?.current?.id)
    }

    func networkChanged(free: Bool) {
        if free {
            begin()
        } else if automatic, unmeteredOnly, running {
            stop()
        }
    }

    func landed(_ track: UInt64) {
        guard mayAsk else {
            seek(track)
            return
        }
        start(Self.everything, stoppingFirst: false, seeking: track)
    }

    func lookAgainForCovers() {
        Task {
            do {
                let native = try await library.player()
                let albums = try await Work.aside { try native.askAgainForCovers() }
                coversAskedAgain = albums
                start(Self.everything, stoppingFirst: true, seeking: library.snapshot?.current?.id)
            } catch {
                library.report(error)
            }
        }
    }

    func stopForErasing() async {
        erased = true
        watching?.cancel()
        watching = nil
        guard let native = library.native else { return }
        _ = try? await Work.aside { native.stopEnrichment() }
    }

    func stop() {
        watching?.cancel()
        watching = nil
        Task {
            guard let native = try? await library.player(),
                let read = try? await Work.aside({
                    native.stopEnrichment()
                    return native.enrichmentStatus()
                })
            else { return }
            publish(read)
            await settle(native)
        }
    }

    func playing(_ id: UInt64?) {
        guard let id, id != sought else { return }
        sought = id
        if running { seek(id) }
    }

    func foregroundChanged(_ active: Bool) {
        guard active != foreground else { return }
        foreground = active
        guard active else {
            watching?.cancel()
            watching = nil
            return
        }
        read()
    }

    func read() {
        Task {
            guard let native = try? await library.player(),
                let read = try? await Work.aside({ native.enrichmentStatus() })
            else { return }
            publish(read)
            await library.features.refreshChangedArt(native)
            if read.running {
                watch(native)
            } else if unsettled {
                await settle(native)
            }
        }
    }

    private func seek(_ track: UInt64) {
        Task {
            guard let native = try? await library.player() else { return }
            _ = try? await Work.aside { native.seekEnrichment(track: track) }
        }
    }

    private func start(_ options: EnrichmentOptions, stoppingFirst: Bool, seeking track: UInt64?) {
        guard !erased else { return }
        Task {
            guard let native = try? await library.player(),
                let read = try? await Work.aside({
                    if stoppingFirst { native.stopEnrichment() }
                    _ = native.startEnrichment(options: options)
                    if let track { native.seekEnrichment(track: track) }
                    return native.enrichmentStatus()
                })
            else { return }
            publish(read)
            if read.running { watch(native) }
        }
    }

    private func watch(_ native: MobilePlayer) {
        guard foreground, watching == nil else { return }
        watching = Task { [weak self] in
            await self?.follow(native)
        }
    }

    private func follow(_ native: MobilePlayer) async {
        while !Task.isCancelled {
            try? await Task.sleep(for: Self.pollEvery)
            guard !Task.isCancelled, let read = try? await Work.aside({ native.enrichmentStatus() }) else { return }
            publish(read)
            await library.features.refreshChangedArt(native)
            guard read.running else {
                watching = nil
                await settle(native)
                return
            }
        }
    }

    private func settle(_ native: MobilePlayer) async {
        unsettled = false
        await library.refresh()
        await library.features.refreshChangedArt(native)
    }

    private func publish(_ read: EnrichmentStatus) {
        if read.running { unsettled = true }
        if status != read { status = read }
        if read.portraits != portraits {
            portraits = read.portraits
            library.features.portraitsChanged()
        }
    }

    private static func line(of status: EnrichmentStatus?) -> String {
        guard let status else { return "Not looked up yet" }
        if status.running {
            return status.due > 0
                ? "Finding artwork and details · \(status.asked.formatted()) of \(status.due.formatted())"
                : "Finding artwork and details"
        }
        return switch status.stopped {
        case .finished: "Artwork and details up to date"
        case .cancelled: "Stopped"
        case .offline: "Stopped: offline"
        case .unreachable: "Stopped: MusicBrainz could not be reached"
        case .refused: "Stopped: the service refused too many requests"
        case .failed: "Stopped: the library could not be read"
        case nil: "Not looked up yet"
        }
    }
}

struct EnrichmentSection: View {
    @Environment(Library.self) private var library

    var body: some View {
        @Bindable var enrichment = library.features.enrichment
        Section {
            Toggle(isOn: $enrichment.automatic) {
                ToolRow(
                    title: "Fetch artwork and details automatically",
                    detail: "Covers, artist portraits, lyrics and MusicBrainz details",
                    symbol: "photo.on.rectangle.angled"
                )
            }
            Toggle(isOn: $enrichment.unmeteredOnly) {
                ToolRow(title: "Only on Wi-Fi", detail: "Wait for a network that is not metered", symbol: "wifi")
            }
            .disabled(!enrichment.automatic)
            Button { enrichment.lookAgainForCovers() } label: {
                ToolRow(
                    title: "Look again for missing covers",
                    detail: enrichment.coversAskedAgain.map { "Asking again for \(counted(Int($0), "album"))" }
                        ?? "Ask again for albums still without a cover",
                    symbol: "photo.badge.arrow.down"
                )
            }
            EnrichmentProgressRow()
        } header: {
            Text("Artwork and details")
        }
        .task { enrichment.read() }
    }
}

struct EnrichmentProgressRow: View {
    @Environment(Library.self) private var library

    var body: some View {
        let enrichment = library.features.enrichment
        HStack(spacing: 12) {
            if enrichment.running { ProgressView() }
            Text(enrichment.line)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .monospacedDigit()
            Spacer()
            if enrichment.running {
                Button("Stop", role: .destructive) { enrichment.stop() }
                    .buttonStyle(.glass)
            }
        }
    }
}

struct RefreshArtworkRow: View {
    @Environment(Library.self) private var library
    @State private var confirming = false

    var body: some View {
        let features = library.features
        let enrichment = features.enrichment
        let songs = library.catalogCount
        HStack {
            Button { confirming = true } label: {
                ToolRow(
                    title: "Refresh artwork and details",
                    detail: enrichment.running
                        ? enrichment.line
                        : !features.online ? "Enable online features in Settings" : "Look up covers, details and lyrics for \(counted(songs, "song")) again",
                    symbol: "wand.and.stars"
                )
            }
            .disabled(!features.online || songs == 0)
            if enrichment.running {
                Spacer()
                Button("Stop", role: .destructive) { enrichment.stop() }
                    .buttonStyle(.glass)
            }
        }
        .alert("Refresh artwork and details?", isPresented: $confirming) {
            Button("Cancel", role: .cancel) {}
            Button("Refresh") { enrichment.refresh() }
        } message: {
            Text("Covers, artist portraits, details and lyrics are looked up again for \(counted(songs, "song")). This runs in the background and can take a while.")
        }
    }
}
