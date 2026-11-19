import ResonateLogic
import ResonateNative
import SwiftUI

struct LibraryView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let arrangement: LibraryArrangement
    @State private var selected = SelectedSongs()
    @State private var indicator = ShelfIndicator()
    @State private var reveal = LibrarySearchReveal()

    var body: some View {
        let search = library.librarySearch
        let shelf = navigation.libraryShelf
        LibraryShelfContent(shelf: shelf)
            .overlay(alignment: .top) { LibrarySearchField(shelf: shelf) }
            .safeAreaBar(edge: .top, spacing: 0) {
                if arrangement == .stack { LibraryShelfRow(indicator: indicator) }
            }
            .environment(reveal)
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
        .onChange(of: shelf) {
            selected.clear()
            reveal.startOver()
        }
        .onChange(of: library.filter) { selected.clear() }
        .onChange(of: search.isAsked) { selected.clear() }
        .onChange(of: library.catalogEmpty) { selected.clear() }
    }
}

struct ScrollPlace: Equatable {
    let offset: Double
    let range: Double
}

@MainActor
@Observable
final class LibrarySearchReveal {
    private(set) var shown = false
    private(set) var band: CGFloat = 0
    @ObservationIgnored private var reveal = SearchReveal()

    func scrolled(_ place: ScrollPlace) {
        reveal.scrolled(to: place.offset, within: place.range)
        if shown != reveal.shown { shown = reveal.shown }
    }

    func startOver() {
        reveal.startOver()
        if shown { shown = false }
    }

    func measured(band height: CGFloat) {
        if band != height { band = height }
    }
}

private struct RevealsLibrarySearch: ViewModifier {
    @Environment(LibrarySearchReveal.self) private var reveal: LibrarySearchReveal?

    func body(content: Content) -> some View {
        content.onScrollGeometryChange(for: ScrollPlace.self) { geometry in
            let insets = geometry.contentInsets
            return ScrollPlace(
                offset: Double(geometry.contentOffset.y + insets.top),
                range: Double(geometry.contentSize.height + insets.top + insets.bottom - geometry.containerSize.height)
            )
        } action: { _, place in
            reveal?.scrolled(place)
        }
    }
}

extension View {
    func revealsLibrarySearch() -> some View {
        modifier(RevealsLibrarySearch())
    }
}

struct LibrarySearchField: View {
    private static let slides: Animation = .spring(duration: 0.32, bounce: 0)
    private static let recentsShown = 6

    @Environment(Library.self) private var library
    @Environment(LibrarySearchReveal.self) private var reveal
    @Environment(\.accessibilityVoiceOverEnabled) private var voiceOver
    @FocusState private var focused: Bool
    let shelf: LibraryShelf

    var body: some View {
        @Bindable var search = library.librarySearch
        let shown = reveal.shown || focused || !search.text.isEmpty || voiceOver
        VStack(spacing: 0) {
            field(search)
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { reveal.measured(band: $0) }
            if focused, search.text.isEmpty, !search.recents.isEmpty {
                recents(search)
                    .padding(.horizontal, 16)
                    .transition(.opacity)
            }
        }
        .offset(y: shown ? 0 : -reveal.band)
        .opacity(shown ? 1 : 0)
        .allowsHitTesting(shown)
        .accessibilityHidden(!shown)
        .motion(Self.slides, calm: Arrival.animation, value: shown)
        .motion(Arrival.animation, value: focused)
    }

    private func field(_ search: SearchModel) -> some View {
        @Bindable var search = search
        return HStack(spacing: 8) {
            HStack(spacing: 6) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
                TextField("search.title", text: $search.text)
                    .focused($focused)
                    .submitLabel(.search)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .onSubmit { remember(search) }
                if !search.text.isEmpty {
                    Button {
                        search.text = ""
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.secondary)
                            .touchTarget()
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("library.search.clear")
                }
            }
            .padding(.leading, 14)
            .padding(.trailing, search.text.isEmpty ? 14 : 0)
            .frame(minHeight: 44)
            .glassEffect(.regular.interactive(), in: .capsule)
            if focused {
                Button("common.cancel") {
                    search.text = ""
                    focused = false
                }
                .buttonStyle(.plain)
                .font(.body)
                .foregroundStyle(Color.worn)
                .transition(.opacity)
            }
        }
    }

    private func recents(_ search: SearchModel) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("search.recents")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.secondary)
                    .accessibilityAddTraits(.isHeader)
                Spacer()
                Button { search.forgetRecents() } label: {
                    Text("search.recents.clear-short").touchTarget()
                }
                .buttonStyle(.plain)
                .font(.subheadline)
                .foregroundStyle(Color.worn)
            }
            .padding(.leading, 16)
            .padding(.trailing, 8)
            ForEach(search.recents.prefix(Self.recentsShown), id: \.self) { recent in
                Button {
                    search.text = recent
                    remember(search)
                } label: {
                    Label {
                        Text(recent).foregroundStyle(.primary).lineLimit(1)
                    } icon: {
                        Image(systemName: "clock.arrow.circlepath").foregroundStyle(.secondary).accessibilityHidden(true)
                    }
                    .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                    .padding(.horizontal, 16)
                    .contentShape(.rect)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.vertical, 6)
        .glassEffect(.regular, in: .rect(cornerRadius: 22, style: .continuous))
    }

    private func remember(_ search: SearchModel) {
        if shelf != .playlists { search.remember() }
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
                    .revealsLibrarySearch()
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

@MainActor
@Observable
final class ShelfIndicator {
    var shown: LibraryShelf?
    var lens: CGFloat?
}

struct LibraryShelfRow: View {
    private static let between: CGFloat = 4
    private static let dragsPast: CGFloat = 8
    private static let slides: Animation = .spring(duration: 0.38, bounce: 0.24)
    private static let held: CGFloat = 1.06
    private static let afterTheSwap: Duration = .milliseconds(16)

    @Environment(Navigation.self) private var navigation
    @Environment(\.dynamicTypeSize) private var typeSize
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let indicator: ShelfIndicator
    @State private var width: CGFloat = 0
    @GestureState private var held = false

    var body: some View {
        let indicated = indicator.shown ?? navigation.libraryShelf
        Group {
            if typeSize.isAccessibilitySize {
                ScrollViewReader { reader in
                    ScrollView(.horizontal, showsIndicators: false) {
                        tabs(indicated, stretched: false)
                    }
                    .clipShape(.capsule)
                    .onChange(of: indicated) { _, now in reader.scrollTo(now, anchor: .center) }
                    .onAppear { reader.scrollTo(indicated, anchor: .center) }
                }
            } else {
                tabs(indicated, stretched: true)
                    .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
                    .simultaneousGesture(slide)
            }
        }
        .padding(Self.between)
        .glassEffect(.regular, in: .capsule)
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .task(id: navigation.libraryShelf) { await follow(navigation.libraryShelf) }
        .onChange(of: held) { _, holding in
            if !holding, indicator.lens != nil { slide(to: indicator.shown ?? navigation.libraryShelf) }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("library.shelves.spoken")
        .accessibilityAddTraits(.isTabBar)
    }

    private var drag: ShelfRowDrag {
        ShelfRowDrag(width: Double(width), count: LibraryShelf.allCases.count, between: Double(Self.between))
    }

    private func lit(_ indicated: LibraryShelf) -> LibraryShelf {
        guard let lens = indicator.lens else { return indicated }
        return LibraryShelf.allCases[drag.nearest(to: Double(lens))]
    }

    private var slide: some Gesture {
        DragGesture(minimumDistance: Self.dragsPast)
            .updating($held) { _, holding, _ in holding = true }
            .onChanged { change in
                let following = CGFloat(drag.clamped(Double(change.location.x)))
                if indicator.lens == nil {
                    withAnimation(reduceMotion ? nil : Self.slides) { indicator.lens = following }
                } else {
                    indicator.lens = following
                }
            }
            .onEnded { change in
                choose(LibraryShelf.allCases[drag.nearest(to: Double(change.location.x))])
            }
    }

    private func choose(_ shelf: LibraryShelf) {
        if shelf == navigation.libraryShelf {
            slide(to: shelf)
        } else {
            navigation.choose(shelf)
        }
    }

    private func follow(_ shelf: LibraryShelf) async {
        guard let shown = indicator.shown else {
            indicator.shown = shelf
            return
        }
        guard shown != shelf || indicator.lens != nil else { return }
        try? await Task.sleep(for: Self.afterTheSwap)
        guard !Task.isCancelled else { return }
        slide(to: shelf)
    }

    private func slide(to shelf: LibraryShelf) {
        withAnimation(reduceMotion ? nil : Self.slides) {
            indicator.lens = nil
            indicator.shown = shelf
        }
    }

    private func tabs(_ indicated: LibraryShelf, stretched: Bool) -> some View {
        let lit = lit(indicated)
        return HStack(spacing: Self.between) {
            ForEach(LibraryShelf.allCases) { offered in
                tab(offered, lit: offered == lit, chosen: offered == indicated, stretched: stretched)
            }
        }
        .backgroundPreferenceValue(ShelfTabBounds.self) { bounds in
            GeometryReader { proxy in
                if let anchor = bounds[indicated] {
                    let rect = proxy[anchor]
                    Capsule()
                        .fill(Color.worn)
                        .frame(width: rect.width, height: rect.height)
                        .scaleEffect(indicator.lens == nil || reduceMotion ? 1 : Self.held)
                        .position(x: indicator.lens ?? rect.midX, y: rect.midY)
                }
            }
        }
    }

    private func tab(_ offered: LibraryShelf, lit: Bool, chosen: Bool, stretched: Bool) -> some View {
        Button {
            choose(offered)
        } label: {
            Text(offered.title)
                .font(.subheadline.weight(.semibold))
                .lineLimit(1)
                .minimumScaleFactor(stretched ? 0.8 : 1)
                .padding(.horizontal, stretched ? 0 : 16)
                .frame(maxWidth: stretched ? .infinity : nil, minHeight: 36)
                .foregroundStyle(lit ? Color.wornInk : Color.primary)
                .contentShape(.capsule)
        }
        .buttonStyle(.plain)
        .anchorPreference(key: ShelfTabBounds.self, value: .bounds) { [offered: $0] }
        .accessibilityAddTraits(chosen ? .isSelected : [])
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
    @Environment(LibrarySearchReveal.self) private var reveal: LibrarySearchReveal?
    let scope: SearchScope

    var body: some View {
        let search = library.librarySearch
        List {
            SearchResults(search: search, scope: .constant(scope)) { Keyboard.stow() }.equatable().themedRows()
        }
        .listStyle(.plain)
        .contentMargins(.top, reveal?.band ?? 0, for: .scrollContent)
        .scrollDismissesKeyboard(.immediately)
        .revealsLibrarySearch()
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
