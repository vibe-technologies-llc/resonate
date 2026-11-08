import ResonateLogic
import ResonateNative
import SwiftUI

private enum SearchArea: String, CaseIterable {
    case all = "All"
    case library = "Library"
    case online = "Online"
}

struct SearchTab: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @State private var seeAll: SearchScope?
    @State private var actions = OnlineSongActions()
    @State private var savingSearch = false
    @State private var area = SearchArea.all

    var body: some View {
        let search = library.tabSearch
        let online = library.features.online

        List {
            Group {
                if search.query.isEmpty {
                    SearchStart(search: search, choose: enter)
                } else {
                    let link = OnlineAsked.isLink(search.query)
                    if !link, area != .online {
                        SearchHeading(title: "In your library")
                        LibraryFindings(search: search, seeAll: $seeAll, compact: area == .all, chosen: stowKeyboard)
                            .equatable()
                        if area == .all, search.hasAnswer, !search.matchedNothing {
                            Button { area = .library } label: {
                                Label("All library results", systemImage: "chevron.forward")
                                    .font(.subheadline)
                            }
                            .listRowSeparator(.hidden)
                        }
                    }
                    if link || area != .library {
                        SearchHeading(title: "Online")
                        if online {
                            OnlineSearchContent(actions: $actions)
                        } else {
                            OnlineOffRow()
                        }
                    }
                }
            }
            .themedRows()
        }
        .listStyle(.plain)
        .safeAreaBar(edge: .top, spacing: 0) {
            if !search.query.isEmpty, !OnlineAsked.isLink(search.query) {
                SearchAreaChooser(area: $area)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
            }
        }
        .scrollDismissesKeyboard(.immediately)
        .searchable(text: Binding(get: { search.text }, set: enter), prompt: "Search")
        .textInputAutocapitalization(.never)
        .autocorrectionDisabled()
        .onSubmit(of: .search) { remember(search) }
        .navigationTitle("Search")
        .navigationDestination(item: $seeAll) { LibrarySearchPage(scope: $0) }
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                DownloadsButton()
            }
            if !search.query.isEmpty, !OnlineAsked.isLink(search.query) {
                ToolbarItem(placement: .topBarTrailing) {
                    Button { savingSearch = true } label: { Label("Save as playlist", systemImage: "bookmark") }
                }
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button { navigation.listening = true } label: { Label("Listen", systemImage: "ear") }
            }
        }
        .sheet(isPresented: $savingSearch) {
            SavedSearchSheet(
                title: "Save search as playlist",
                draft: .starting(name: search.query, text: search.query)
            ) { name, saved in
                remember(search)
                library.features.saveSearch(name, search: saved) { problem in
                    if let problem {
                        library.fail(problem)
                    } else {
                        library.notice("Saved \(name) among your playlists")
                    }
                }
            }
        }
        .onlineSongActions($actions)
        .onAppear {
            search.recallRecents()
            library.features.onlineQuery = search.text
        }
        .onChange(of: search.text) { _, text in library.features.onlineQuery = text }
    }

    private func stowKeyboard() {
        Keyboard.stow()
    }

    private func enter(_ text: String) {
        library.tabSearch.text = text
        library.features.onlineQuery = text
    }

    private func remember(_ search: SearchModel) {
        guard !OnlineAsked.isLink(search.query) else { return }
        search.remember()
    }
}

struct SearchHeading: View {
    let title: String

    var body: some View {
        Text(title)
            .font(.title3.weight(.bold))
            .listRowSeparator(.hidden)
            .accessibilityAddTraits(.isHeader)
    }
}

struct OnlineOffRow: View {
    @Environment(Navigation.self) private var navigation

    var body: some View {
        Button { navigation.tab = .settings } label: {
            HStack {
                Label("Turn on online features to search online too", systemImage: "globe")
                    .foregroundStyle(.secondary)
                Spacer(minLength: 0)
                Image(systemName: "chevron.forward")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
            }
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .listRowSeparator(.hidden)
        .accessibilityHint("Opens Settings")
    }
}

struct SearchStart: View {
    @Environment(Navigation.self) private var navigation
    let search: SearchModel
    let choose: (String) -> Void

    var body: some View {
        Section("Discover") {
            Button { navigation.listening = true } label: {
                ToolRow(title: "Recognise a song", detail: "Name the music playing around you", symbol: "ear")
            }
            .buttonStyle(.plain)
            NavigationLink(value: Destination.wanted) {
                ToolRow(title: "Saved for later", detail: "Find and download the songs you saved", symbol: "bookmark")
            }
        }
        if !search.recents.isEmpty {
            Section {
                ForEach(search.recents, id: \.self) { recent in
                    Button { choose(recent) } label: {
                        Label {
                            Text(recent).foregroundStyle(.primary)
                        } icon: {
                            Image(systemName: "clock.arrow.circlepath").foregroundStyle(.secondary).accessibilityHidden(true)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(.rect)
                    }
                    .buttonStyle(.plain)
                }
            } header: {
                HStack {
                    Text("Recent searches")
                    Spacer()
                    Button { search.forgetRecents() } label: {
                        Text("Clear").touchTarget()
                    }
                    .font(.subheadline)
                    .textCase(nil)
                }
            }
        }
        ContentUnavailableView(
            "Search your library and online",
            systemImage: "magnifyingglass",
            description: Text("Find songs, albums and artists you have and music online to download, or paste a shared link to open it.")
        )
        .listRowSeparator(.hidden)
    }
}

struct LibraryFindings: View, Equatable {
    let search: SearchModel
    @Binding var seeAll: SearchScope?
    let compact: Bool
    let chosen: () -> Void
    private let searched: ObjectIdentifier

    init(search: SearchModel, seeAll: Binding<SearchScope?>, compact: Bool = false, chosen: @escaping () -> Void) {
        self.search = search
        _seeAll = seeAll
        self.compact = compact
        self.chosen = chosen
        searched = ObjectIdentifier(search)
    }

    nonisolated static func == (shown: LibraryFindings, other: LibraryFindings) -> Bool {
        shown.searched == other.searched && shown.compact == other.compact
    }

    var body: some View {
        SearchCompletions(search: search)
        if search.matchedNothing {
            if let meant = search.found.didYouMean {
                Button { search.text = meant } label: {
                    Label {
                        Text("Did you mean \(Text(meant).bold())?")
                    } icon: {
                        Image(systemName: "text.magnifyingglass")
                            .accessibilityHidden(true)
                    }
                }
            }
            Text("Nothing in your library matches “\(search.answered)”")
                .foregroundStyle(.secondary)
                .listRowSeparator(.hidden)
                .announced("Nothing in your library matches \(search.answered)")
        } else {
            SearchResults(
                search: search,
                scope: Binding(get: { .top }, set: { seeAll = $0 }),
                chosen: chosen,
                songsAtTop: compact ? 3 : 5,
                showsCollections: !compact
            )
            .equatable()
        }
    }
}

private struct SearchAreaChooser: View {
    @Binding var area: SearchArea
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Namespace private var chosen

    var body: some View {
        HStack(spacing: 4) {
            ForEach(SearchArea.allCases, id: \.self) { offered in
                let isChosen = offered == area
                Button {
                    withAnimation(reduceMotion ? nil : Flip.animation) { area = offered }
                } label: {
                    Text(offered.rawValue)
                        .font(.subheadline.weight(.semibold))
                        .lineLimit(1)
                        .minimumScaleFactor(0.8)
                        .frame(maxWidth: .infinity, minHeight: 36)
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
            }
        }
        .padding(4)
        .glassEffect(.regular, in: .capsule)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Search in")
    }
}

struct LibrarySearchPage: View {
    @Environment(Library.self) private var library
    let scope: SearchScope

    var body: some View {
        List {
            SearchResults(search: library.tabSearch, scope: .constant(scope)).equatable().themedRows()
        }
        .listStyle(.plain)
        .navigationTitle(scope.rawValue)
    }
}
