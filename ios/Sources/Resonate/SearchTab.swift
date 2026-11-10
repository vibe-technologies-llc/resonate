import ResonateLogic
import ResonateNative
import SwiftUI

private enum SearchArea: CaseIterable {
    case all
    case library
    case online

    var label: String {
        switch self {
        case .all: said("search.area.all")
        case .library: said("search.area.library")
        case .online: said("search.area.online")
        }
    }
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
                        SearchHeading(title: said("search.heading.library"))
                        LibraryFindings(search: search, seeAll: $seeAll, compact: area == .all, chosen: stowKeyboard)
                            .equatable()
                        if area == .all, search.hasAnswer, !search.matchedNothing {
                            Button { area = .library } label: {
                                Label("search.all-library-results", systemImage: "chevron.forward")
                                    .font(.subheadline)
                            }
                            .listRowSeparator(.hidden)
                        }
                    }
                    if link || area != .library {
                        SearchHeading(title: said("search.heading.online"))
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
        .searchable(text: Binding(get: { search.text }, set: enter), prompt: "search.title")
        .textInputAutocapitalization(.never)
        .autocorrectionDisabled()
        .onSubmit(of: .search) { remember(search) }
        .navigationTitle("search.title")
        .navigationDestination(item: $seeAll) { LibrarySearchPage(scope: $0) }
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                DownloadsButton()
            }
            if !search.query.isEmpty, !OnlineAsked.isLink(search.query) {
                ToolbarItem(placement: .topBarTrailing) {
                    Button { savingSearch = true } label: { Label("search.save-as-playlist", systemImage: "bookmark") }
                }
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button { navigation.listening = true } label: { Label("search.listen", systemImage: "ear") }
            }
        }
        .sheet(isPresented: $savingSearch) {
            SavedSearchSheet(
                title: said("search.save-as-playlist.title"),
                draft: .starting(name: search.query, text: search.query)
            ) { name, saved in
                remember(search)
                library.features.saveSearch(name, search: saved) { problem in
                    if let problem {
                        library.fail(problem)
                    } else {
                        library.notice(said("search.saved-as-playlist", name))
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
                Label("search.online-off", systemImage: "globe")
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
        .accessibilityHint("search.online-off.hint")
    }
}

struct SearchStart: View {
    @Environment(Navigation.self) private var navigation
    let search: SearchModel
    let choose: (String) -> Void

    var body: some View {
        Section("search.discover") {
            Button { navigation.listening = true } label: {
                ToolRow(title: said("search.discover.recognise"), detail: said("search.discover.recognise.detail"), symbol: "ear")
            }
            .buttonStyle(.plain)
            NavigationLink(value: Destination.wanted) {
                ToolRow(title: said("search.discover.wanted"), detail: said("search.discover.wanted.detail"), symbol: "bookmark")
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
                    Text("search.recents")
                    Spacer()
                    Button { search.forgetRecents() } label: {
                        Text("search.recents.clear-short").touchTarget()
                    }
                    .font(.subheadline)
                    .textCase(nil)
                }
            }
        }
        ContentUnavailableView(
            "search.start.title",
            systemImage: "magnifyingglass",
            description: Text("search.start.detail")
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
                        Text(NothingFound.meaning(meant))
                    } icon: {
                        Image(systemName: "text.magnifyingglass")
                            .accessibilityHidden(true)
                    }
                }
            }
            Text(said("search.library.nothing", said("common.quoted", search.answered)))
                .foregroundStyle(.secondary)
                .listRowSeparator(.hidden)
                .announced(said("search.library.nothing", search.answered))
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
                    Text(offered.label)
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
        .accessibilityLabel("search.area.spoken")
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
        .navigationTitle(scope.label)
    }
}
