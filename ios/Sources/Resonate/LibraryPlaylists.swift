import ResonateLogic
import ResonateNative
import SwiftUI

struct LibraryPlaylists: View {
    private static let narrowest: CGFloat = 150
    private static let between: CGFloat = 12

    @Environment(Library.self) private var library
    @Environment(\.dynamicTypeSize) private var typeSize
    @State private var naming = false
    @State private var importing = false
    @State private var unfollowing: String?

    var body: some View {
        @Bindable var library = library
        let features = library.features
        let asked = library.librarySearch.query
        let folded = asked.searchFolded
        let playlists = library.playlistOrder.sorted(features.playlists.filter { folded.isEmpty || features.foldedName(of: $0).contains(folded) })
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if folded.isEmpty {
                    ShortcutCards {
                        NavigationLink(value: Destination.favourites) {
                            FavouritesCard()
                        }
                        .buttonStyle(.shortcutCard)
                        NavigationLink(value: Destination.party) {
                            PartyCard()
                        }
                        .buttonStyle(.shortcutCard)
                    }
                    .environment(\.plainPage, true)
                }
                Group {
                    ShelfHeader(title: said("library.playlists.yours")) {
                        if features.playlists.count > 1 { PlaylistOrderMenu(order: $library.playlistOrder) }
                    }
                    if playlists.isEmpty {
                        none(asked: asked, folded: folded)
                    } else if typeSize.isAccessibilitySize {
                        LazyVStack(alignment: .leading, spacing: 16) {
                            ForEach(playlists, id: \.id) { playlist in
                                NavigationLink(value: Destination.playlist(playlist.id)) {
                                    PlaylistShelfRow(playlist: playlist)
                                }
                                .buttonStyle(.plain)
                                .collectionActions(.playlist(playlist.id))
                            }
                        }
                    } else {
                        LazyVGrid(
                            columns: [GridItem(.adaptive(minimum: Self.narrowest), spacing: Self.between, alignment: .top)],
                            spacing: 16
                        ) {
                            ForEach(playlists, id: \.id) { PlaylistCard(playlist: $0) }
                        }
                    }
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
        }
        .scrollDismissesKeyboard(.immediately)
        .toolbar {
            if !features.playlists.isEmpty {
                ToolbarItem(placement: .topBarLeading) {
                    Button("playlists.undo", systemImage: "arrow.uturn.backward") {
                        features.undoPlaylistEdit()
                    }
                }
            }
            ToolbarItem(placement: .topBarTrailing) {
                Menu("playlists.new", systemImage: "plus") {
                    Button("playlists.create", systemImage: "plus") { naming = true }
                    Button("playlists.import-file", systemImage: "square.and.arrow.down") { importing = true }
                    Button("playlists.paste-link", systemImage: "doc.on.clipboard") {
                        features.together.accept(UIPasteboard.general.string ?? "")
                    }
                }
            }
            if !features.followedProfiles.isEmpty {
                ToolbarItem(placement: .topBarTrailing) {
                    Menu("playlists.followed-profiles", systemImage: "person.2") {
                        Section(FollowedProfileWords.stopFollowingHeading) {
                            ForEach(features.followedProfiles, id: \.link) { profile in
                                Button(FollowedProfileWords.followed(profile)) { unfollowing = profile.link }
                            }
                        }
                    }
                }
            }
            ToolbarSpacer(.fixed, placement: .topBarTrailing)
            LibraryToolbar()
        }
        .fileImporter(isPresented: $importing, allowedContentTypes: PlaylistFileTypes.offered) { result in
            switch result {
            case .success(let url): library.importPlaylistFile(url)
            case .failure(let failure): library.report(failure)
            }
        }
        .confirmationDialog(
            unfollowing.map { link in
                FollowedProfileWords.stopFollowing(features.followedProfiles.first { $0.link == link }?.name ?? link)
            } ?? "",
            isPresented: Binding(get: { unfollowing != nil }, set: { if !$0 { unfollowing = nil } }),
            titleVisibility: .visible,
            presenting: unfollowing
        ) { link in
            Button("playlists.stop-following", role: .destructive) { features.unfollowProfile(link) }
        } message: { _ in
            Text(FollowedProfileWords.stoppingExplained)
        }
        .alert("playlists.create", isPresented: $naming) {
            PlaylistNameField(confirm: said("common.create")) { features.createPlaylist($0) }
        }
    }

    @ViewBuilder
    private func none(asked: String, folded: String) -> some View {
        if folded.isEmpty {
            ContentUnavailableView {
                Label("library.playlists.none", systemImage: "music.note.list")
            } description: {
                Text("library.playlists.invitation")
            } actions: {
                Button("playlists.new") { naming = true }
                    .buttonStyle(.glass)
            }
        } else {
            Text(said("library.playlists.none-matching", asked))
                .foregroundStyle(.secondary)
        }
    }
}

struct PartyCard: View {
    var body: some View {
        ShortcutCard(title: said("library.party"), detail: said("library.party.detail"), symbol: "person.2.wave.2")
    }
}

struct PlaylistCard: View {
    @Environment(Library.self) private var library
    let playlist: MusicPlaylist

    var body: some View {
        NavigationLink(value: Destination.playlist(playlist.id)) {
            VStack(alignment: .leading, spacing: 4) {
                Color.clear
                    .aspectRatio(1, contentMode: .fit)
                    .overlay { PlaylistArtwork(playlist: playlist, corner: 12) }
                Text(playlist.name)
                    .font(.subheadline.weight(.medium))
                    .titleLines()
                Text(detail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .titleLines()
            }
            .contentShape(.rect)
            .accessibilityElement(children: .combine)
        }
        .buttonStyle(.plain)
        .collectionActions(.playlist(playlist.id))
    }

    private var detail: String {
        let shared = SharedPlaylistWords.kind(shared: library.features.together.shared(playlist.id)?.mayEdit, followed: playlist.followed)
        let kind = shared ?? (playlist.search == nil ? nil : said("playlist.saved-search"))
        return [kind, said("counted.songs", Int(playlist.songs))].compactMap { $0 }.joined(separator: " · ")
    }
}
