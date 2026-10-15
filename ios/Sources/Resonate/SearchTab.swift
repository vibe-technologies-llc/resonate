import ResonateNative
import SwiftUI

enum SearchPlace: String, CaseIterable, Identifiable {
    case library = "Your library"
    case online = "Online"

    var id: String { rawValue }
}

struct SearchTab: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @State private var seeAll: SearchScope?
    @State private var actions = OnlineSongActions()

    var body: some View {
        @Bindable var navigation = navigation
        let search = library.tabSearch
        let place = navigation.searchPlace

        List {
            Group {
                if place == .online, !library.features.online {
                    OnlineOff()
                } else if search.query.isEmpty {
                    SearchStart(search: search, choose: enter)
                } else {
                    switch place {
                    case .library:
                        LibraryFindings(search: search, seeAll: $seeAll) { navigation.searchPlace = .online }
                    case .online:
                        OnlineSearchContent(actions: $actions)
                    }
                }
            }
            .themedRows()
        }
        .listStyle(.plain)
        .searchable(text: Binding(get: { search.text }, set: enter), prompt: place == .online ? OnlineAsked.prompt : "Search your library")
        .searchScopes($navigation.searchPlace) {
            ForEach(SearchPlace.allCases) { Text($0.rawValue).tag($0) }
        }
        .searchSuggestions {
            if place == .library { SearchSuggestions(search: search, offersRecents: false) }
        }
        .textInputAutocapitalization(.never)
        .autocorrectionDisabled()
        .onSubmit(of: .search) { remember(search) }
        .navigationTitle("Search")
        .navigationDestination(item: $seeAll) { LibrarySearchPage(scope: $0) }
        .toolbar {
            if place == .library, search.asking, search.hasAnswer {
                ToolbarItem(placement: .topBarTrailing) { ProgressView() }
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button { navigation.listening = true } label: { Label("Listen", systemImage: "ear") }
            }
        }
        .onlineSongActions($actions)
        .onChange(of: place) { _, now in
            if now == .online { library.features.onlineQuery = search.text }
        }
        .onAppear {
            search.recallRecents()
            if place == .online { library.features.onlineQuery = search.text }
        }
    }

    private func enter(_ text: String) {
        library.tabSearch.text = text
        if navigation.searchPlace == .online { library.features.onlineQuery = text }
    }

    private func remember(_ search: SearchModel) {
        guard !OnlineAsked.isLink(search.query) else { return }
        search.remember()
    }
}

struct SearchStart: View {
    let search: SearchModel
    let choose: (String) -> Void

    var body: some View {
        if !search.recents.isEmpty {
            Section {
                ForEach(search.recents, id: \.self) { recent in
                    Button { choose(recent) } label: {
                        Label {
                            Text(recent).foregroundStyle(.primary)
                        } icon: {
                            Image(systemName: "clock.arrow.circlepath").foregroundStyle(.secondary)
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
                    Button("Clear") { search.forgetRecents() }
                        .font(.subheadline)
                        .textCase(nil)
                }
            }
        }
        ContentUnavailableView(
            "Search your library and online",
            systemImage: "magnifyingglass",
            description: Text("Find songs, albums and artists you have, or choose Online to find music to download.")
        )
        .listRowSeparator(.hidden)
    }
}

struct LibraryFindings: View {
    private static let few = 5

    let search: SearchModel
    @Binding var seeAll: SearchScope?
    let searchOnline: () -> Void

    var body: some View {
        SearchResults(search: search, scope: Binding(get: { .top }, set: { seeAll = $0 }))
        if search.hasAnswer, search.found.trackCount < Self.few {
            Section {
                Button(action: searchOnline) {
                    Label("Search online for “\(search.query)”", systemImage: "globe")
                }
            }
        }
    }
}

struct LibrarySearchPage: View {
    @Environment(Library.self) private var library
    let scope: SearchScope

    var body: some View {
        List {
            SearchResults(search: library.tabSearch, scope: .constant(scope)).themedRows()
        }
        .listStyle(.plain)
        .navigationTitle(scope.rawValue)
    }
}
