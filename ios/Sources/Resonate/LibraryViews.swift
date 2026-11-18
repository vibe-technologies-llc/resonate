import ResonateLogic
import ResonateNative
import SwiftUI

struct LibraryView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let arrangement: LibraryArrangement
    @State private var selected = SelectedSongs()

    var body: some View {
        @Bindable var search = library.librarySearch
        let shelf = navigation.libraryShelf
        LibraryShelfContent(shelf: shelf)
            .safeAreaBar(edge: .top, spacing: 0) {
                if arrangement == .stack { LibraryShelfRow() }
            }
            .searchable(text: $search.text, placement: .navigationBarDrawer(displayMode: .always), prompt: "search.title")
            .searchSuggestions { SearchSuggestions(search: search) }
            .onSubmit(of: .search) {
                if shelf != .playlists { search.remember() }
            }
        .refreshable {
            library.rescan()
            library.features.loadPlaylists()
        }
        .modifier(LibraryTitled(arrangement: arrangement, shelf: shelf))
        .environment(selected)
        .onAppear {
            search.recallRecents()
            library.features.loadPlaylists()
        }
        .onChange(of: shelf) { selected.clear() }
        .onChange(of: library.filter) { selected.clear() }
        .onChange(of: search.isAsked) { selected.clear() }
        .onChange(of: library.catalogEmpty) { selected.clear() }
    }
}

private struct LibraryTitled: ViewModifier {
    @Environment(SelectedSongs.self) private var selected
    let arrangement: LibraryArrangement
    let shelf: LibraryShelf

    func body(content: Content) -> some View {
        content
            .navigationTitle(selected.selecting ? said("selection.count", selected.count) : title)
            .navigationBarTitleDisplayMode(.inline)
    }

    private var title: String {
        arrangement == .split ? shelf.title : said("library.title")
    }
}

struct LibraryShelfContent: View {
    @Environment(Library.self) private var library
    let shelf: LibraryShelf

    var body: some View {
        Group {
            switch shelf {
            case .playlists:
                LibraryPlaylists()
            case _ where library.catalogEmpty:
                List { EmptyLibrary().themedRows() }
                    .listStyle(.plain)
                    .toolbar { LibraryToolbar() }
            case .songs:
                LibrarySongs()
            case .albums:
                LibraryAlbums().toolbar { LibraryToolbar() }
            case .artists:
                LibraryArtists().toolbar { LibraryToolbar() }
            }
        }
        .transaction(value: shelf) { $0.animation = nil }
    }
}

struct LibraryShelfRow: View {
    private static let between: CGFloat = 4

    @Environment(Navigation.self) private var navigation
    @Environment(\.dynamicTypeSize) private var typeSize
    @Namespace private var chosen

    var body: some View {
        let shelf = navigation.libraryShelf
        Group {
            if typeSize.isAccessibilitySize {
                ScrollViewReader { reader in
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: Self.between) { tabs(shelf, stretched: false) }
                    }
                    .clipShape(.capsule)
                    .onChange(of: shelf) { _, now in reader.scrollTo(now, anchor: .center) }
                    .onAppear { reader.scrollTo(shelf, anchor: .center) }
                }
            } else {
                HStack(spacing: Self.between) { tabs(shelf, stretched: true) }
            }
        }
        .motion(Flip.animation, value: shelf)
        .padding(Self.between)
        .glassEffect(.regular, in: .capsule)
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("library.shelves.spoken")
        .accessibilityAddTraits(.isTabBar)
    }

    private func tabs(_ shelf: LibraryShelf, stretched: Bool) -> some View {
        ForEach(LibraryShelf.allCases) { offered in
            let isChosen = offered == shelf
            Button {
                navigation.choose(offered)
            } label: {
                Text(offered.title)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
                    .minimumScaleFactor(stretched ? 0.8 : 1)
                    .padding(.horizontal, stretched ? 0 : 16)
                    .frame(maxWidth: stretched ? .infinity : nil, minHeight: 36)
                    .foregroundStyle(isChosen ? Color.wornInk : Color.primary)
                    .background {
                        if isChosen {
                            Capsule()
                                .fill(Color.worn)
                                .matchedGeometryEffect(id: "chosen", in: chosen)
                        }
                    }
                    .contentShape(.capsule)
            }
            .buttonStyle(.plain)
            .accessibilityAddTraits(isChosen ? .isSelected : [])
            .id(offered)
        }
    }
}

struct ShelfHeader<Controls: View>: View {
    let title: String
    var detail: String?
    @ViewBuilder let controls: Controls

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 8) {
                words
                Spacer(minLength: 8)
                controls
            }
            VStack(alignment: .leading, spacing: 8) {
                words
                HStack(spacing: 8) { controls }
            }
        }
    }

    private var words: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title)
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
            if let detail {
                Text(detail)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
    }
}

struct LibraryFound: View {
    @Environment(Library.self) private var library
    let scope: SearchScope

    var body: some View {
        let search = library.librarySearch
        List {
            SearchCompletions(search: search).themedRows()
            SearchResults(search: search, scope: .constant(scope)) { Keyboard.stow() }.equatable().themedRows()
        }
        .listStyle(.plain)
        .scrollDismissesKeyboard(.immediately)
        .environment(SelectedSongs?.none)
    }
}

struct ShelfStanding: View {
    @Environment(Library.self) private var library
    let none: String
    let detail: String
    let symbol: String

    var body: some View {
        if let unread = library.collectionsUnread {
            ReadFailure(text: unread) { library.reloadCollections() }
        } else if !library.collectionsRead {
            ProgressView()
                .frame(maxWidth: .infinity)
                .accessibilityLabel("library.reading")
        } else {
            ContentUnavailableView(none, systemImage: symbol, description: Text(detail))
        }
    }
}
