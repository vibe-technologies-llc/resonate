import ResonateLogic
import ResonateNative
import SwiftUI

enum DeletionFailure: LocalizedError {
    case filesKept(Int)

    var errorDescription: String? {
        switch self {
        case .filesKept(let count):
            "\(counted(count, "file")) could not be deleted; \(count == 1 ? "its folder" : "their folders") would not let go of \(count == 1 ? "it" : "them")"
        }
    }
}

struct SongsToDelete: Identifiable {
    let id = UUID()
    let tracks: [Track]

    var title: String {
        tracks.count == 1 ? "Delete “\(tracks[0].shownTitle)”?" : "Delete \(counted(tracks.count, "song"))?"
    }
}

extension Library {
    func deleteSongs(_ tracks: [Track]) {
        let folders = folders
        let doomed = tracks.map { DoomedFile(id: $0.id, uri: $0.uri) }
        Task { [weak self] in
            guard let self, let native = try? await player() else { return }
            let gone = (try? await Work.aside { doomed.filter { folders.deleteSong($0.uri) }.map(\.id) }) ?? []
            let told: SongsForgotten
            do {
                told = try await Work.aside { try native.forgetDeletedSongs(ids: gone) }
            } catch {
                report(error)
                return
            }
            await songsLeft(told)
            if gone.count < doomed.count {
                report(DeletionFailure.filesKept(doomed.count - gone.count))
            } else {
                notice(tracks.count == 1 ? "Deleted “\(tracks[0].shownTitle)”" : "Deleted \(counted(tracks.count, "song"))")
            }
        }
    }

    func deleteIdenticalCopies(_ sets: [IdenticalSongs]) async {
        guard let native = try? await player() else { return }
        let folders = folders
        var folded: UInt32 = 0
        var leftTheQueue: UInt32 = 0
        var stuck = 0
        var unfolded: (any Swift.Error)?

        for set in sets {
            let copies = set.copies.map { DoomedFile(id: $0.id, uri: $0.uri) }
            let gone = (try? await Work.aside { copies.filter { folders.deleteSong($0.uri) }.map(\.id) }) ?? []
            stuck += copies.count - gone.count
            guard !gone.isEmpty else { continue }
            let kept = set.kept.id
            do {
                let told = try await Work.aside { try native.foldIdenticalSongs(kept: kept, copies: gone) }
                folded += told.forgotten
                leftTheQueue += told.leftTheQueue
            } catch {
                unfolded = unfolded ?? error
            }
        }
        await songsLeft(SongsForgotten(forgotten: folded, leftTheQueue: leftTheQueue))
        if let unfolded, stuck > 0 {
            report(unfolded, after: explained(DeletionFailure.filesKept(stuck)))
        } else if let unfolded {
            report(unfolded)
        } else if stuck > 0 {
            report(DeletionFailure.filesKept(stuck))
        } else if folded > 0 {
            notice("Deleted \(counted(Int(folded), "identical file")); each song keeps one")
        }
    }

    private func songsLeft(_ told: SongsForgotten) async {
        if told.leftTheQueue > 0 { await rereadQueue() }
        guard told.forgotten > 0 else { return }
        await refresh()
        await features.reloadOpenPlaylist()
        features.loadPlaylists()
    }
}

private struct DoomedFile: Sendable {
    let id: UInt64
    let uri: String
}

struct DeleteSongsConfirmation: ViewModifier {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let insidePlayer: Bool

    func body(content: Content) -> some View {
        let asked = navigation.deleting
        content.confirmationDialog(
            asked?.title ?? "",
            isPresented: Binding(
                get: { asked != nil && navigation.nowPlaying == insidePlayer },
                set: { if !$0 { navigation.deleting = nil } }
            ),
            titleVisibility: .visible,
            presenting: asked
        ) { asked in
            Button(asked.tracks.count == 1 ? "Delete song" : "Delete \(counted(asked.tracks.count, "song"))", role: .destructive) {
                library.deleteSongs(asked.tracks)
            }
        } message: { asked in
            Text(asked.tracks.count == 1
                ? "Its file is deleted from your iPhone, and it leaves the queue, your playlists and favourites."
                : "Their files are deleted from your iPhone, and they leave the queue, your playlists and favourites.")
        }
    }
}

struct IdenticalSongsView: View {
    @Environment(Library.self) private var library
    @State private var found: [IdenticalSongs]?
    @State private var failure: String?
    @State private var chosen: [UInt64: UInt64] = [:]
    @State private var confirming = false
    @State private var deleting = false
    @State private var asked = 0

    var body: some View {
        let sets = kept
        let copies = sets.reduce(0) { $0 + $1.copies.count }
        ThemedList {
            if let failure {
                ReadFailure(text: failure) { asked += 1 }
            } else if let found {
                if found.isEmpty {
                    ContentUnavailableView(
                        "No identical songs",
                        systemImage: "checkmark.circle",
                        description: Text("No two songs in your library are the same file.")
                    )
                    .listRowSeparator(.hidden)
                }
                ForEach(found, id: \.kept.id) { set in
                    Section {
                        ForEach(members(of: set), id: \.id) { track in
                            IdenticalCopyRow(track: track, kept: keptId(of: set) == track.id) {
                                chosen[set.kept.id] = track.id
                            }
                        }
                    } footer: {
                        Text("Tap the copy to keep. The others are deleted.")
                    }
                }
            } else {
                HStack(spacing: 12) {
                    ProgressView()
                    Text("Comparing songs…").foregroundStyle(.secondary)
                }
            }
        }
        .navigationTitle("Identical songs")
        .toolbar {
            if copies > 0 {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Delete copies", role: .destructive) { confirming = true }
                        .disabled(deleting)
                }
            }
        }
        .confirmationDialog(
            "Delete \(counted(copies, "file"))?",
            isPresented: $confirming,
            titleVisibility: .visible
        ) {
            Button("Delete \(counted(copies, "file"))", role: .destructive) { delete(sets) }
        } message: {
            Text("Each song keeps one file. The other files are deleted from your iPhone, and their plays, favourites and playlist places move to the copy kept.")
        }
        .task(id: asked) { await find() }
    }

    private var kept: [IdenticalSongs] {
        (found ?? []).map { set in
            let members = members(of: set)
            let keeping = keptId(of: set)
            return IdenticalSongs(
                kept: members.first { $0.id == keeping } ?? set.kept,
                copies: members.filter { $0.id != keeping }
            )
        }
    }

    private func members(of set: IdenticalSongs) -> [Track] {
        [set.kept] + set.copies
    }

    private func keptId(of set: IdenticalSongs) -> UInt64 {
        chosen[set.kept.id] ?? set.kept.id
    }

    private func find() async {
        guard let native = try? await library.player() else { return }
        do {
            found = try await Work.aside { try native.identicalSongs() }
            failure = nil
            chosen = [:]
        } catch {
            failure = explained(error)
        }
    }

    private func delete(_ sets: [IdenticalSongs]) {
        deleting = true
        Task {
            await library.deleteIdenticalCopies(sets)
            deleting = false
            asked += 1
        }
    }
}

private struct IdenticalCopyRow: View {
    @Environment(Library.self) private var library
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 44
    let track: Track
    let kept: Bool
    let keep: () -> Void

    var body: some View {
        Button(action: keep) {
            HStack(spacing: 12) {
                ArtworkView(track: track, corner: 6)
                    .frame(width: artworkEdge, height: artworkEdge)
                VStack(alignment: .leading, spacing: 2) {
                    Text(track.shownTitle).lineLimit(1)
                    Text(place).font(.subheadline).foregroundStyle(.secondary).lineLimit(2)
                }
                Spacer(minLength: 0)
                Image(systemName: kept ? "checkmark.circle.fill" : "trash")
                    .foregroundStyle(kept ? Color.worn : Color.secondary)
                    .accessibilityHidden(true)
            }
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .accessibilityValue(kept ? "Kept" : "Deleted")
        .accessibilityHint(kept ? "" : "Keeps this copy instead")
    }

    private var place: String {
        let root = Folders.root(of: track.uri)
        let folder = library.folderList.first { $0.id == root }
        let named = [folder?.name, track.name].compactMap { $0 }.joined(separator: " › ")
        return folder?.writable == false ? named + " · read-only" : named
    }
}
