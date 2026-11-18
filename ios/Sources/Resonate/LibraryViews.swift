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
            .searchable(text: $search.text, prompt: shelf.searchPrompt)
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
        content.navigationTitle(selected.selecting ? said("selection.count", selected.count) : title)
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
    private static let indicator: CGFloat = 3
    private static let gutter: CGFloat = 12

    @Environment(Navigation.self) private var navigation
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.dynamicTypeSize) private var typeSize
    @Namespace private var chosen

    var body: some View {
        let shelf = navigation.libraryShelf
        Group {
            if typeSize.isAccessibilitySize {
                ScrollViewReader { reader in
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 0) { tabs(shelf, stretched: false) }
                    }
                    .onChange(of: shelf) { _, now in reader.scrollTo(now, anchor: .center) }
                    .onAppear { reader.scrollTo(shelf, anchor: .center) }
                }
            } else {
                HStack(spacing: 0) { tabs(shelf, stretched: true) }
            }
        }
        .padding(.horizontal, 4)
        .overlay(alignment: .bottom) { Divider() }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("library.shelves.spoken")
        .accessibilityAddTraits(.isTabBar)
    }

    private func tabs(_ shelf: LibraryShelf, stretched: Bool) -> some View {
        ForEach(LibraryShelf.allCases) { offered in
            let isChosen = offered == shelf
            Button {
                withAnimation(reduceMotion ? nil : Flip.animation) { navigation.choose(offered) }
            } label: {
                Text(offered.title)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
                    .foregroundStyle(isChosen ? Color.worn : Color.secondary)
                    .padding(.horizontal, Self.gutter)
                    .frame(maxWidth: stretched ? .infinity : nil, minHeight: 44)
                    .overlay(alignment: .bottom) {
                        if isChosen {
                            Capsule()
                                .fill(Color.worn)
                                .frame(height: Self.indicator)
                                .padding(.horizontal, Self.gutter)
                                .matchedGeometryEffect(id: "chosen", in: chosen)
                        }
                    }
                    .contentShape(.rect)
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
