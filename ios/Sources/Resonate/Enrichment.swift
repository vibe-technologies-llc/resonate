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
    @ObservationIgnored private var away = UIApplication.shared.applicationState == .background
    @ObservationIgnored private var owedOnComingForward = false
    @ObservationIgnored private var lane: Task<Void, Never>?
    @ObservationIgnored private lazy var pausing = BackgroundHold(name: said("enrichment.background.pausing"))
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

    private var everything: EnrichmentOptions {
        EnrichmentOptions(refresh: false, atMost: nil, acoustidKey: library.features.recognitionKey)
    }

    private var refreshing: EnrichmentOptions {
        EnrichmentOptions(refresh: true, atMost: nil, acoustidKey: library.features.recognitionKey)
    }

    var line: String { Self.line(of: status) }

    private var mayAsk: Bool {
        let features = library.features
        return automatic && features.online && (!unmeteredOnly || features.unmetered)
    }

    func begin() {
        guard mayAsk else { return }
        start(everything, stoppingFirst: false, seeking: library.snapshot?.current?.id)
    }

    func refresh() {
        start(refreshing, stoppingFirst: true, seeking: library.snapshot?.current?.id)
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
        start(everything, stoppingFirst: false, seeking: track)
    }

    func lookAgainForCovers() {
        Task {
            do {
                let native = try await library.player()
                let albums = try await Work.aside { try native.askAgainForCovers() }
                coversAskedAgain = albums
                start(everything, stoppingFirst: true, seeking: library.snapshot?.current?.id)
            } catch {
                library.report(error)
            }
        }
    }

    func stopForErasing() async {
        erased = true
        watching?.cancel()
        watching = nil
        await lane?.value
        guard let native = library.native else { return }
        _ = try? await Work.aside { native.stopEnrichment() }
    }

    func stop() {
        watching?.cancel()
        watching = nil
        owedOnComingForward = false
        inTurn { [self] in
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

    func wentToBackground() {
        away = true
        pausing.hold(true)
        inTurn { [self] in
            defer { pausing.hold(false) }
            guard away, let native = library.native,
                let before = try? await Work.aside({ native.enrichmentStatus() }), before.running,
                let read = try? await Work.aside({
                    native.pauseEnrichment()
                    return native.enrichmentStatus()
                })
            else { return }
            owedOnComingForward = true
            publish(read)
            carryOn()
        }
    }

    func cameForward() {
        away = false
        carryOn()
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

    private func carryOn() {
        guard owedOnComingForward, !away else { return }
        owedOnComingForward = false
        guard !automatic || !unmeteredOnly || library.features.unmetered else { return }
        start(everything, stoppingFirst: false, seeking: library.snapshot?.current?.id)
    }

    private func inTurn(_ step: @escaping @MainActor () async -> Void) {
        let before = lane
        lane = Task {
            await before?.value
            await step()
        }
    }

    private func start(_ options: EnrichmentOptions, stoppingFirst: Bool, seeking track: UInt64?) {
        inTurn { [self] in
            guard !erased else { return }
            guard !away else {
                owedOnComingForward = true
                return
            }
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
        guard let status else { return said("enrichment.status.not-yet") }
        if status.running {
            return status.due > 0
                ? said("enrichment.status.finding.counted", status.asked.formatted(), status.due.formatted())
                : said("enrichment.status.finding")
        }
        return switch status.stopped {
        case .finished: said("enrichment.status.finished")
        case .cancelled: said("enrichment.status.cancelled")
        case .offline: said("enrichment.status.offline")
        case .unreachable: said("enrichment.status.unreachable")
        case .refused: said("enrichment.status.refused")
        case .failed: said("enrichment.status.failed")
        case nil: said("enrichment.status.not-yet")
        }
    }
}

struct EnrichmentView: View {
    @Environment(Library.self) private var library

    var body: some View {
        let features = library.features
        ThemedForm {
            if !features.online {
                Section {
                    Toggle(isOn: Binding(get: { features.online }, set: { features.setOnline($0) })) {
                        ToolRow(title: said("enrichment.online"), detail: said("enrichment.online.detail"), symbol: "globe")
                    }
                }
            }
            EnrichmentSection()
                .disabled(!features.online)
        }
        .navigationTitle("enrichment.title")
    }
}

struct EnrichmentSection: View {
    @Environment(Library.self) private var library

    var body: some View {
        @Bindable var enrichment = library.features.enrichment
        Section {
            Toggle(isOn: $enrichment.automatic) {
                ToolRow(
                    title: said("enrichment.automatic"),
                    detail: said("enrichment.automatic.detail"),
                    symbol: "photo.on.rectangle.angled"
                )
            }
            Toggle(isOn: $enrichment.unmeteredOnly) {
                ToolRow(title: said("enrichment.unmetered"), detail: said("enrichment.unmetered.detail"), symbol: "wifi")
            }
            .disabled(!enrichment.automatic)
            Toggle(isOn: Binding(get: { library.features.enrichOnMetered }, set: { library.features.setEnrichOnMetered($0) })) {
                ToolRow(title: said("enrichment.metered"), detail: said("enrichment.metered.detail"), symbol: "antenna.radiowaves.left.and.right")
            }
            Button { enrichment.lookAgainForCovers() } label: {
                ToolRow(
                    title: said("enrichment.covers-again"),
                    detail: enrichment.coversAskedAgain.map { said("enrichment.covers-again.asking", Int($0)) }
                        ?? said("enrichment.covers-again.detail"),
                    symbol: "photo.badge.arrow.down"
                )
            }
            EnrichmentProgressRow()
        } footer: {
            Text("enrichment.footer")
        }
        .task { enrichment.read() }
    }
}

struct EnrichmentProgressRow: View {
    @Environment(Library.self) private var library

    var body: some View {
        let enrichment = library.features.enrichment
        HStack(spacing: 12) {
            if enrichment.running { ProgressView().accessibilityHidden(true) }
            Text(enrichment.line)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .monospacedDigit()
            Spacer()
            if enrichment.running {
                Button("common.stop", role: .destructive) { enrichment.stop() }
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
                    title: said("enrichment.refresh"),
                    detail: enrichment.running
                        ? enrichment.line
                        : !features.online ? said("enrichment.refresh.offline") : said("enrichment.refresh.detail", songs),
                    symbol: "wand.and.stars"
                )
            }
            .disabled(!features.online || songs == 0)
            if enrichment.running {
                Spacer()
                Button("common.stop", role: .destructive) { enrichment.stop() }
                    .buttonStyle(.glass)
            }
        }
        .alert("enrichment.refresh.confirm", isPresented: $confirming) {
            Button("common.cancel", role: .cancel) {}
            Button("enrichment.refresh.go") { enrichment.refresh() }
        } message: {
            Text(said("enrichment.refresh.message", songs))
        }
    }
}
