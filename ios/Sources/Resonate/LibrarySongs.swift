import ResonateLogic
import ResonateNative
import SwiftUI

enum SongsList {
    case searched
    case browsed
}

struct LibrarySongs: View {
    @Environment(Library.self) private var library

    var body: some View {
        if library.librarySearch.isAsked {
            LibraryFound(scope: .songs)
                .toolbar { LibraryToolbar() }
                .id(SongsList.searched)
        } else {
            SongsBrowsed()
                .id(SongsList.browsed)
        }
    }
}

struct EarlierRows: Hashable {
    let before: Int
}

struct SongsBrowsed: View {
    private static let landsAfter = Duration.milliseconds(30)

    @Environment(Library.self) private var library
    @Environment(SelectedSongs.self) private var selected
    @State private var window = RowWindow()
    @State private var track = ScrollTrack()
    @State private var landingOn: UInt64?
    @State private var landing: Task<Void, Never>?

    var body: some View {
        let tracks = library.tracks
        let rows = window.rows(of: tracks.count)
        let selecting = selected.selecting
        ScrollViewReader { reader in
            List {
                SongsLead(playable: !tracks.isEmpty)
                    .listRowSeparator(.hidden)
                    .themedRows()
                listed(tracks, rows, in: reader)
                    .themedRows()
            }
            .listStyle(.plain)
            .revealsLibrarySearch()
            .onScrollGeometryChange(for: Double.self) { geometry in
                ScrollTrack.share(of: geometry)
            } action: { _, share in
                track.moved(to: share)
            }
            .overlay(alignment: .trailing) {
                FastScroller(track: track, rows: tracks.count, window: rows) { row in
                    jump(to: row, of: tracks, in: reader)
                }
            }
        }
        .safeAreaBar(edge: .bottom, spacing: 0) {
            if selecting { SongSelectionBar() }
        }
        .toolbar {
            if selecting {
                ToolbarItem(placement: .topBarLeading) {
                    Button("selection.clear", systemImage: "xmark") { selected.clear() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("selection.all") { selected.selectAll(library.tracks) }
                }
            } else {
                if !tracks.isEmpty {
                    ToolbarItem(placement: .topBarTrailing) {
                        Button("common.select", systemImage: "checkmark.circle") { selected.begin() }
                    }
                    ToolbarSpacer(.fixed, placement: .topBarTrailing)
                }
                LibraryToolbar()
            }
        }
        .onChange(of: library.filter) { window = RowWindow() }
        .onChange(of: tracks.count) {
            if selected.selecting { selected.retain(library.tracks) }
        }
    }

    @ViewBuilder
    private func listed(_ tracks: [Track], _ rows: Range<Int>, in reader: ScrollViewProxy) -> some View {
        if tracks.isEmpty, !library.tracksRead {
            ProgressView()
                .frame(maxWidth: .infinity)
                .accessibilityLabel("library.reading")
                .listRowSeparator(.hidden)
        } else if tracks.isEmpty, library.filtering {
            ContentUnavailableView {
                Label("songs.filtered.none", systemImage: "line.3.horizontal.decrease.circle")
            } description: {
                Text("songs.filtered.none.detail")
            } actions: {
                Button("songs.filtered.reset") { library.setFilter(Library.everything) }
                    .buttonStyle(.glass)
            }
            .listRowSeparator(.hidden)
        } else {
            if window.holdsEarlier {
                MoreRows { widenEarlier(tracks, rows, in: reader) }
                    .id(EarlierRows(before: rows.lowerBound))
            }
            ForEach(tracks[rows], id: \.id) { track in
                TrackRow(track: track) { library.play(tracks, selected: track) }
            }
            if window.holdsMore(than: tracks.count) {
                MoreRows { window.widen() }
            }
        }
    }

    private func widenEarlier(_ tracks: [Track], _ rows: Range<Int>, in reader: ScrollViewProxy) {
        guard !track.grabbed, landing == nil, rows.lowerBound < tracks.count else { return }
        let wasFirst = tracks[rows.lowerBound].id

        window.widenEarlier()
        Task { reader.scrollTo(wasFirst, anchor: .top) }
    }

    private func jump(to row: Int, of tracks: [Track], in reader: ScrollViewProxy) {
        guard row < tracks.count else { return }

        window.anchor(at: row, of: tracks.count)
        landingOn = tracks[row].id
        guard landing == nil else { return }
        landing = Task {
            try? await Task.sleep(for: Self.landsAfter)
            if let landingOn { reader.scrollTo(landingOn, anchor: .top) }
            landing = nil
        }
    }
}

struct SongsLead: View {
    @Environment(Library.self) private var library
    @Environment(SelectedSongs.self) private var selected
    let playable: Bool

    var body: some View {
        if playable, !selected.selecting {
            HStack(spacing: 12) {
                CollectionAction(title: said("common.play"), symbol: "play.fill", prominent: true) {
                    library.play(library.tracks, at: 0)
                }
                CollectionAction(title: said("common.shuffle"), symbol: "shuffle", prominent: false) {
                    library.shuffle(library.tracks)
                }
            }
            .listRowInsets(EdgeInsets(top: 12, leading: 16, bottom: 8, trailing: 16))
        }
        ShelfHeader(title: said("counted.songs", library.tracks.count), detail: length) {
            SongOrderMenu(ordering: ordering, asChip: true)
            FilterButton()
        }
        .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))
        FilterChips()
    }

    private var length: String? {
        let minutes = library.tracksLengthMs / 60_000
        guard minutes > 0 else { return nil }
        return minutes >= 60 ? said("collection.length.hours", Int(minutes / 60), Int(minutes % 60)) : said("collection.length.minutes", Int(minutes))
    }

    private var ordering: Binding<SongOrdering> {
        Binding(
            get: { SongOrdering(order: library.filter.order, descending: library.filter.descending) },
            set: { chosen in
                var filter = library.filter
                filter.order = chosen.order
                filter.descending = chosen.descending
                library.setFilter(filter)
            }
        )
    }
}
