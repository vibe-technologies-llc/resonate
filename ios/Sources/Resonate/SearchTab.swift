import ResonateLogic
import ResonateNative
import SwiftUI

struct SearchTab: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @State private var seeAll: SearchScope?
    @State private var actions = OnlineSongActions()

    var body: some View {
        let search = library.tabSearch
        let online = library.features.online

        List {
            Group {
                if search.query.isEmpty {
                    SearchStart(search: search, choose: enter)
                } else {
                    if !OnlineAsked.isLink(search.query) {
                        SearchHeading(title: "In your library")
                        LibraryFindings(search: search, seeAll: $seeAll, chosen: stowKeyboard)
                    }
                    SearchHeading(title: "Online")
                    if online {
                        OnlineSearchContent(actions: $actions)
                    } else {
                        OnlineOffRow()
                    }
                }
            }
            .themedRows()
        }
        .listStyle(.plain)
        .scrollDismissesKeyboard(.immediately)
        .searchable(text: Binding(get: { search.text }, set: enter), prompt: "Search")
        .textInputAutocapitalization(.never)
        .autocorrectionDisabled()
        .onSubmit(of: .search) { remember(search) }
        .navigationTitle("Search")
        .navigationDestination(item: $seeAll) { LibrarySearchPage(scope: $0) }
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button { navigation.listening = true } label: { Label("Listen", systemImage: "ear") }
            }
        }
        .onlineSongActions($actions)
        .onAppear {
            search.recallRecents()
            library.features.onlineQuery = search.text
        }
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
                Image(systemName: "chevron.forward").font(.footnote.weight(.semibold)).foregroundStyle(.tertiary)
            }
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .listRowSeparator(.hidden)
        .accessibilityHint("Opens Settings")
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

struct LibraryFindings: View {
    let search: SearchModel
    @Binding var seeAll: SearchScope?
    let chosen: () -> Void

    var body: some View {
        SearchCompletions(search: search)
        if search.matchedNothing {
            if let meant = search.found.didYouMean {
                Button { search.text = meant } label: {
                    Label {
                        Text("Did you mean \(Text(meant).bold())?")
                    } icon: {
                        Image(systemName: "text.magnifyingglass")
                    }
                }
            }
            Text("Nothing in your library matches “\(search.answered)”")
                .foregroundStyle(.secondary)
                .listRowSeparator(.hidden)
        } else {
            SearchResults(search: search, scope: Binding(get: { .top }, set: { seeAll = $0 }), chosen: chosen)
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
