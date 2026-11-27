import ResonateLogic
import ResonateNative
import SwiftUI
import UIKit

enum RecompressFailure: LocalizedError {
    case unwritten(String)

    var errorDescription: String? {
        switch self {
        case .unwritten(let title): said("storage.recompress.unwritten", title)
        }
    }
}

enum StorageSizes {
    @MainActor static let settings = UserDefaults(suiteName: "library-features") ?? .standard

    static func shown(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: bytes), countStyle: .file)
    }
}

struct RecompressedSong: Sendable {
    let outcome: RecompressOutcome
    let track: Track?
}

extension Library {
    func recompress(_ track: Track, to ceiling: RecompressCeiling) async -> RecompressedSong? {
        guard let native = try? await player() else { return nil }
        let folders = folders
        let id = track.id
        let uri = track.uri
        let title = track.shownTitle
        return await betweenScans {
            await Self.recompressing(native, folders, id: id, uri: uri, title: title, ceiling: ceiling)
        }
    }

    nonisolated private static func recompressing(
        _ native: MobilePlayer,
        _ folders: Folders,
        id: UInt64,
        uri: String,
        title: String,
        ceiling: RecompressCeiling
    ) async -> RecompressedSong {
        let staged: RecompressStaged
        do {
            staged = try await Work.aside { try native.recompressSong(id: id, ceiling: ceiling) }
        } catch {
            return RecompressedSong(outcome: .failed(explained(error)), track: nil)
        }
        switch staged {
        case .keptOriginal(_, let said):
            return RecompressedSong(outcome: .kept(said), track: nil)
        case .staged(let stagePath, let before, let after):
            let stage = URL(fileURLWithPath: stagePath)
            let stamp: FileStamp
            do {
                stamp = try await Work.aside { try folders.replace(uri, withCopyOf: stage) }
            } catch {
                _ = try? await Work.aside { native.forgetRecompressStage(stagePath: stagePath) }
                return RecompressedSong(outcome: .failed(explained(RecompressFailure.unwritten(title))), track: nil)
            }
            do {
                let indexed = try await Work.inLibrary {
                    try native.songRecompressed(id: id, stagePath: stagePath, modifiedMs: stamp.modifiedMs)
                }
                return RecompressedSong(outcome: .shrunk(before: before, after: after), track: indexed)
            } catch {
                _ = try? await Work.aside { native.forgetRecompressStage(stagePath: stagePath) }
                return RecompressedSong(outcome: .failed(explained(error)), track: nil)
            }
        }
    }
}

@MainActor
@Observable
final class Recompressing {
    private(set) var running = false
    private(set) var stopping = false
    private(set) var total = 0
    private(set) var tally = RecompressTally()
    private(set) var current: String?
    private(set) var resized: [SizedSong] = []
    @ObservationIgnored private var holding: UIBackgroundTaskIdentifier = .invalid

    func one(_ track: Track, to ceiling: RecompressTo, in library: Library) {
        begin(library) { [weak self] in
            guard let self else { return }
            total = 1
            guard let outcome = await song(track, ceiling, library) else { return }
            let told = outcome.told(title: track.shownTitle, size: StorageSizes.shown)
            if case .failed = outcome { library.fail(told) } else { library.notice(told) }
        }
    }

    func every(to ceiling: RecompressTo, in library: Library) {
        begin(library) { [weak self] in
            guard let self else { return }
            let songs: [Track]
            do {
                songs = try await Self.flacs(library)
            } catch {
                library.report(error)
                return
            }
            total = songs.count
            for track in songs {
                if stopping { break }
                guard await song(track, ceiling, library) != nil else { break }
            }
            let summary = tally.told(stopped: stopping, size: StorageSizes.shown)
            if let first = tally.firstFailure {
                library.fail(said("storage.told.first-failure", summary, first))
            } else {
                library.notice(summary)
            }
        }
    }

    func stop() {
        guard running else { return }
        stopping = true
    }

    private func song(_ track: Track, _ ceiling: RecompressTo, _ library: Library) async -> RecompressOutcome? {
        current = track.shownTitle
        guard let done = await library.recompress(track, to: ceiling.ceiling) else { return nil }
        tally.add(done.outcome)
        if case .shrunk(_, let after) = done.outcome, let indexed = done.track {
            resized.append(SizedSong(track: indexed, bytes: after))
        }
        return done.outcome
    }

    private func begin(_ library: Library, _ run: @escaping @MainActor () async -> Void) {
        guard !running else { return }
        running = true
        stopping = false
        total = 0
        tally = RecompressTally()
        current = nil
        holding = UIApplication.shared.beginBackgroundTask(withName: said("storage.run.background"), expirationHandler: Self.expiry(of: self))
        Task {
            await run()
            await library.refresh()
            finish()
        }
    }

    private func finish() {
        running = false
        stopping = false
        current = nil
        letGo()
    }

    private func expired() {
        stop()
        letGo()
    }

    private func letGo() {
        guard holding != .invalid else { return }
        UIApplication.shared.endBackgroundTask(holding)
        holding = .invalid
    }

    nonisolated private static func expiry(of run: Recompressing) -> @Sendable () -> Void {
        { [weak run] in
            Task { @MainActor in run?.expired() }
        }
    }

    private static func flacs(_ library: Library) async throws -> [Track] {
        let native = try await library.player()
        let rows = StoragePages.pageRows
        let songs = try await Work.aside {
            var pages = StoragePages()
            repeat {
                pages.took(try native.largestSongs(after: pages.after, limit: rows))
            } while !pages.ended
            return pages.songs.map(\.track)
        }
        return songs.filter { StorageRows.recompressible($0) && library.features.songDeletable($0) }
    }
}

struct StorageView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @AppStorage(RecompressTo.key, store: StorageSizes.settings) private var ceiling: RecompressTo = RecompressTo.standing
    @State private var pages = StoragePages()
    @State private var failure: String?
    @State private var reading = false
    @State private var applied = 0
    @State private var confirmingEvery = false
    @State private var confirmingOne: Track?

    var body: some View {
        let run = library.recompressing
        ThemedList {
            Section {
                Picker(selection: $ceiling) {
                    ForEach(RecompressTo.allCases) { choice in
                        Text(choice.title).tag(choice)
                    }
                } label: {
                    Text("storage.ceiling")
                }
                Button { confirmingEvery = true } label: {
                    ToolRow(
                        title: said("storage.recompress-every"),
                        detail: said("storage.recompress-every.detail"),
                        symbol: "arrow.down.right.and.arrow.up.left"
                    )
                }
                .disabled(run.running)
                if run.running {
                    RecompressProgress()
                }
            } footer: {
                Text(said(ceiling.lowersTheResolution ? "storage.ceiling.footer.lowers" : "storage.ceiling.footer.lossless"))
            }
            Section {
                if let failure {
                    ReadFailure(text: failure) { Task { await readMore() } }
                } else if pages.ended, pages.songs.isEmpty {
                    ContentUnavailableView(
                        "storage.none",
                        systemImage: "internaldrive",
                        description: Text("storage.none.detail")
                    )
                    .listRowSeparator(.hidden)
                }
                ForEach(pages.songs, id: \.track.id) { song in
                    StorageSongRow(song: song)
                        .swipeActions(edge: .trailing) { removals(song.track) }
                        .swipeActions(edge: .leading) { recompression(song.track, running: run.running) }
                        .contextMenu {
                            recompression(song.track, running: run.running)
                            removals(song.track)
                        }
                }
                if failure == nil, !pages.ended {
                    HStack(spacing: 12) {
                        ProgressView().accessibilityHidden(true)
                        Text("storage.reading").foregroundStyle(.secondary)
                    }
                    .task(id: pages.songs.count) { await readMore() }
                }
            } header: {
                if !pages.songs.isEmpty {
                    Text(said("storage.listed", pages.songs.count, StorageSizes.shown(pages.listedBytes)))
                }
            }
        }
        .navigationTitle("settings.storage.title")
        .confirmationDialog(
            said("storage.recompress-every.title"),
            isPresented: $confirmingEvery,
            titleVisibility: .visible
        ) {
            Button("storage.recompress.confirm") { run.every(to: ceiling, in: library) }
        } message: {
            Text(said("storage.recompress.message", ceiling.title))
        }
        .confirmationDialog(
            said("storage.recompress-one.title", confirmingOne?.shownTitle ?? ""),
            isPresented: Binding(get: { confirmingOne != nil }, set: { if !$0 { confirmingOne = nil } }),
            titleVisibility: .visible,
            presenting: confirmingOne
        ) { track in
            Button("storage.recompress.confirm") { run.one(track, to: ceiling, in: library) }
        } message: { _ in
            Text(said("storage.recompress.message", ceiling.title))
        }
        .onAppear { applied = run.resized.count }
        .onChange(of: run.resized.count) { _, count in
            for song in run.resized[min(applied, count)..<count] {
                pages.resized(song.track, bytes: song.bytes)
            }
            applied = count
        }
        .onChange(of: library.catalogEpoch) {
            guard library.catalogRead else { return }
            pages.retain { library.track($0) != nil }
        }
    }

    @ViewBuilder
    private func recompression(_ track: Track, running: Bool) -> some View {
        if StorageRows.recompressible(track), library.features.songDeletable(track), !running {
            Button("storage.recompress", systemImage: "arrow.down.right.and.arrow.up.left") {
                if ceiling.lowersTheResolution {
                    confirmingOne = track
                } else {
                    library.recompressing.one(track, to: ceiling, in: library)
                }
            }
            .tint(.indigo)
        }
    }

    @ViewBuilder
    private func removals(_ track: Track) -> some View {
        if library.features.songDeletable(track) {
            Button("song.delete", systemImage: "trash", role: .destructive) {
                navigation.deleting = SongsToDelete(tracks: [track])
            }
            .tint(.red)
            if library.features.syncsWithOtherDevices {
                Button("song.delete-everywhere", systemImage: "trash.slash", role: .destructive) {
                    navigation.deleting = SongsToDelete(tracks: [track], everywhere: true)
                }
                .tint(.red)
            }
        }
    }

    private func readMore() async {
        guard !reading, !pages.ended, let native = try? await library.player() else { return }
        reading = true
        defer { reading = false }
        let after = pages.after
        do {
            let page = try await Work.onScreen { try native.largestSongs(after: after, limit: StoragePages.pageRows) }
            pages.took(page)
            failure = nil
        } catch {
            failure = explained(error)
        }
    }
}

private struct StorageSongRow: View {
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 44
    let song: SizedSong

    var body: some View {
        HStack(spacing: 12) {
            ArtworkView(track: song.track, corner: 6)
                .frame(width: artworkEdge, height: artworkEdge)
            VStack(alignment: .leading, spacing: 2) {
                Text(song.track.shownTitle).titleLines()
                Text(StorageRows.line(song, size: StorageSizes.shown))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .titleLines(2)
            }
            Spacer(minLength: 0)
        }
        .accessibilityElement(children: .combine)
    }
}

private struct RecompressProgress: View {
    @Environment(Library.self) private var library

    var body: some View {
        let run = library.recompressing
        VStack(alignment: .leading, spacing: 6) {
            if run.total > 0 {
                let done = Double(min(run.tally.done, run.total)) / Double(run.total)
                ProgressView(value: done) { Text(said("storage.run.progress", min(run.tally.done + 1, run.total), run.total)) }
            } else {
                ProgressView { Text("storage.run.finding") }
            }
            if let current = run.current {
                Text(current).font(.subheadline).foregroundStyle(.secondary).titleLines()
            }
            if run.tally.saved > 0 {
                Text(said("storage.run.saved", StorageSizes.shown(run.tally.saved)))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            Button(run.stopping ? "storage.run.stopping" : "storage.run.stop", role: .cancel) { run.stop() }
                .disabled(run.stopping)
        }
        .padding(.vertical, 4)
    }
}
