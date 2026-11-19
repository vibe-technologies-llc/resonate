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
            .searchable(text: $search.text, placement: .navigationBarDrawer(displayMode: .automatic), prompt: "search.title")
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

struct ShelfTabBounds: PreferenceKey {
    static let defaultValue: [LibraryShelf: Anchor<CGRect>] = [:]

    static func reduce(value: inout [LibraryShelf: Anchor<CGRect>], nextValue: () -> [LibraryShelf: Anchor<CGRect>]) {
        value.merge(nextValue()) { $1 }
    }
}

struct LibraryShelfRow: View {
    private static let between: CGFloat = 4
    private static let dragsPast: CGFloat = 8
    private static let slides: Animation = .spring(duration: 0.38, bounce: 0.24)
    private static let held: CGFloat = 1.06

    @Environment(Navigation.self) private var navigation
    @Environment(\.dynamicTypeSize) private var typeSize
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var indicated = LibraryShelf.songs
    @State private var lens: CGFloat?
    @State private var width: CGFloat = 0
    @GestureState private var held = false

    var body: some View {
        Group {
            if typeSize.isAccessibilitySize {
                ScrollViewReader { reader in
                    ScrollView(.horizontal, showsIndicators: false) {
                        tabs(stretched: false)
                    }
                    .clipShape(.capsule)
                    .onChange(of: indicated) { _, now in reader.scrollTo(now, anchor: .center) }
                    .onAppear { reader.scrollTo(indicated, anchor: .center) }
                }
            } else {
                tabs(stretched: true)
                    .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
                    .simultaneousGesture(slide)
            }
        }
        .padding(Self.between)
        .glassEffect(.regular, in: .capsule)
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .onAppear { indicated = navigation.libraryShelf }
        .onChange(of: navigation.libraryShelf) { _, now in
            if lens == nil, now != indicated { indicated = now }
        }
        .onChange(of: held) { _, holding in
            if !holding, lens != nil { settle(on: indicated) }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("library.shelves.spoken")
        .accessibilityAddTraits(.isTabBar)
    }

    private var drag: ShelfRowDrag {
        ShelfRowDrag(width: Double(width), count: LibraryShelf.allCases.count, between: Double(Self.between))
    }

    private var lit: LibraryShelf {
        guard let lens else { return indicated }
        return LibraryShelf.allCases[drag.nearest(to: Double(lens))]
    }

    private var slide: some Gesture {
        DragGesture(minimumDistance: Self.dragsPast)
            .updating($held) { _, holding, _ in holding = true }
            .onChanged { change in
                let following = CGFloat(drag.clamped(Double(change.location.x)))
                if lens == nil {
                    withAnimation(reduceMotion ? nil : Self.slides) { lens = following }
                } else {
                    lens = following
                }
            }
            .onEnded { change in
                settle(on: LibraryShelf.allCases[drag.nearest(to: Double(change.location.x))])
            }
    }

    private func settle(on shelf: LibraryShelf) {
        withAnimation(reduceMotion ? nil : Self.slides) {
            lens = nil
            indicated = shelf
        }
        navigation.choose(shelf)
    }

    private func tabs(stretched: Bool) -> some View {
        HStack(spacing: Self.between) {
            ForEach(LibraryShelf.allCases) { offered in
                tab(offered, stretched: stretched)
            }
        }
        .backgroundPreferenceValue(ShelfTabBounds.self) { bounds in
            GeometryReader { proxy in
                if let anchor = bounds[indicated] {
                    let rect = proxy[anchor]
                    Capsule()
                        .fill(Color.worn)
                        .frame(width: rect.width, height: rect.height)
                        .scaleEffect(lens == nil || reduceMotion ? 1 : Self.held)
                        .position(x: lens ?? rect.midX, y: rect.midY)
                }
            }
        }
    }

    private func tab(_ offered: LibraryShelf, stretched: Bool) -> some View {
        let isChosen = offered == lit
        return Button {
            settle(on: offered)
        } label: {
            Text(offered.title)
                .font(.subheadline.weight(.semibold))
                .lineLimit(1)
                .minimumScaleFactor(stretched ? 0.8 : 1)
                .padding(.horizontal, stretched ? 0 : 16)
                .frame(maxWidth: stretched ? .infinity : nil, minHeight: 36)
                .foregroundStyle(isChosen ? Color.wornInk : Color.primary)
                .contentShape(.capsule)
        }
        .buttonStyle(.plain)
        .anchorPreference(key: ShelfTabBounds.self, value: .bounds) { [offered: $0] }
        .accessibilityAddTraits(offered == indicated ? .isSelected : [])
        .id(offered)
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
