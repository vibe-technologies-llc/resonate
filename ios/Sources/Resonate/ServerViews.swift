import ResonateLogic
import ResonateNative
import SwiftUI
import UIKit

extension Library {
    func playServed(expanding uri: String, shuffled: Bool) {
        asking(uri) { uris in
            if shuffled {
                self.shuffleServed(uris)
            } else {
                self.playServed(uris, at: 0)
            }
        }
    }

    func playServedNext(expanding uri: String) {
        asking(uri) { self.playServedNext($0) }
    }

    func addServedToQueue(expanding uri: String) {
        asking(uri) { self.addServedToQueue($0) }
    }

    private func asking(_ uri: String, then: @escaping @MainActor ([String]) -> Void) {
        Task {
            do {
                let uris = try await features.serverAnswer { try $0.servedSongUris(uri: uri) }
                then(uris)
            } catch {
                report(error)
            }
        }
    }
}

struct ServedActions: ViewModifier {
    @Environment(Library.self) private var library
    let uri: String

    func body(content: Content) -> some View {
        content
            .swipeActions(edge: .leading) {
                Button("Play next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                    library.playServedNext(expanding: uri)
                }
                .tint(.blue)
                Button("Add to queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                    library.addServedToQueue(expanding: uri)
                }
                .tint(.indigo)
            }
            .contextMenu {
                Button("Play", systemImage: "play") { library.playServed(expanding: uri, shuffled: false) }
                Button("Shuffle", systemImage: "shuffle") { library.playServed(expanding: uri, shuffled: true) }
                Button("Play next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                    library.playServedNext(expanding: uri)
                }
                Button("Add to queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                    library.addServedToQueue(expanding: uri)
                }
            }
    }
}

extension View {
    func servedActions(_ uri: String) -> some View {
        modifier(ServedActions(uri: uri))
    }
}

struct ServedCoverLoad: Equatable {
    let uri: String
    let bucket: ArtworkBucket
    let generation: Int
}

struct ServedCoverView: View {
    @Environment(Library.self) private var library
    @Environment(\.displayScale) private var displayScale
    let uri: String
    let cover: String?
    let edge: CGFloat
    var circle = false
    var corner: CGFloat = 8
    @State private var image: UIImage?

    var body: some View {
        let load = ServedCoverLoad(
            uri: uri,
            bucket: ArtworkBucket.fitting(points: edge, scale: displayScale),
            generation: library.features.servedGeneration
        )
        Rectangle()
            .fill(.quaternary)
            .overlay {
                Image(systemName: circle ? "person.fill" : "music.note")
                    .font(.title2)
                    .foregroundStyle(.secondary)
            }
            .overlay {
                if let image {
                    Image(uiImage: image)
                        .resizable()
                        .scaledToFill()
                }
            }
            .frame(width: edge, height: edge)
            .modifier(ArtworkClip(circle: circle, corner: corner))
            .task(id: load) {
                let features = library.features
                if let held = features.cachedServedCover(load.uri, bucket: load.bucket) {
                    image = held
                    return
                }
                image = await features.servedCover(load.uri, known: cover, bucket: load.bucket)
            }
            .accessibilityHidden(true)
    }
}

struct ServedRow: View {
    @ScaledMetric(relativeTo: .body) private var edge: CGFloat = 56
    let uri: String
    let cover: String?
    let title: String
    let detail: String?
    var circle = false

    var body: some View {
        HStack(spacing: 12) {
            ServedCoverView(uri: uri, cover: cover, edge: edge, circle: circle)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.body).lineLimit(1)
                if let detail, !detail.isEmpty {
                    Text(detail).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            Spacer(minLength: 0)
        }
        .accessibilityElement(children: .combine)
    }
}

struct ServedSongRow: View {
    @Environment(Library.self) private var library
    @ScaledMetric(relativeTo: .callout) private var numberWidth: CGFloat = 28
    let song: ServedSong
    var number: UInt32?
    let play: () -> Void

    var body: some View {
        Button(action: play) {
            if let number {
                HStack(spacing: 12) {
                    Text("\(number)")
                        .font(.callout.monospacedDigit())
                        .foregroundStyle(.secondary)
                        .frame(width: numberWidth)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(song.shownTitle).font(.body).lineLimit(1)
                        let detail = song.line(withAlbum: false)
                        if !detail.isEmpty {
                            Text(detail).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                        }
                    }
                    Spacer(minLength: 0)
                }
                .contentShape(.rect)
            } else {
                ServedRow(uri: song.uri, cover: song.cover, title: song.shownTitle, detail: song.line(withAlbum: true))
                    .contentShape(.rect)
            }
        }
        .buttonStyle(.plain)
        .servedActions(song.uri)
    }
}

struct ServedFoundRows: View {
    @Environment(Library.self) private var library
    let found: ServedFound

    var body: some View {
        Text(found.summary).font(.footnote).foregroundStyle(.secondary).themedRows()
        if !found.artists.isEmpty {
            Section("Artists") {
                ForEach(found.artists, id: \.uri) { artist in
                    NavigationLink(value: Destination.served(ServedPlace(artist))) {
                        ServedRow(uri: artist.uri, cover: artist.cover, title: artist.shownName, detail: artist.line, circle: true)
                    }
                    .servedActions(artist.uri)
                }
                .themedRows()
            }
        }
        if !found.albums.isEmpty {
            Section("Albums") {
                ForEach(found.albums, id: \.uri) { album in
                    NavigationLink(value: Destination.served(ServedPlace(album))) {
                        ServedRow(uri: album.uri, cover: album.cover, title: album.shownName, detail: album.line)
                    }
                    .servedActions(album.uri)
                }
                .themedRows()
            }
        }
        if !found.songs.isEmpty {
            Section("Songs") {
                let uris = found.songs.map(\.uri)
                ForEach(Array(found.songs.enumerated()), id: \.element.uri) { index, song in
                    ServedSongRow(song: song) { library.playServed(uris, at: index, shuffle: library.snapshot?.shuffle == true) }
                }
                .themedRows()
            }
        }
    }
}

struct ServedPageLoad: Equatable {
    let service: String
    let words: String
    let generation: Int
}

struct ServerView: View {
    @Environment(Library.self) private var library
    @State private var first: ServedFound?
    @State private var firstOf: String?
    @State private var found: ServedFound?
    @State private var typed = ""
    @State private var asked = ""
    @State private var failure: String?
    @State private var searchFailure: String?

    var body: some View {
        let features = library.features
        let generation = features.servedGeneration
        let choice = features.served
        let service = choice.service
        let scheme = service?.scheme ?? ""
        let title = service?.title ?? "Server"
        List {
            if choice.choosing {
                Picker("Service", selection: Binding(get: { scheme }, set: { features.chooseServed($0) })) {
                    ForEach(choice.services, id: \.scheme) { offered in
                        Text(offered.title).tag(offered.scheme)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .themedRows()
            }
            if asked.isEmpty {
                firstRows(scheme: scheme, title: title)
            } else {
                foundRows(scheme: scheme, title: title)
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .navigationTitle(choice.choosing ? "Server" : title)
        .searchable(text: $typed, prompt: "Search \(title)")
        .onSubmit(of: .search) { asked = typed.trimmingCharacters(in: .whitespacesAndNewlines) }
        .onChange(of: typed) { _, now in
            if now.isEmpty { asked = "" }
        }
        .task(id: ServedPageLoad(service: scheme, words: "", generation: generation)) { await loadFirst(scheme) }
        .task(id: ServedPageLoad(service: scheme, words: asked, generation: generation)) { await search(scheme) }
    }

    @ViewBuilder
    private func firstRows(scheme: String, title: String) -> some View {
        if let failure {
            ReadFailure(text: failure) { Task { await loadFirst(scheme) } }
                .themedRows()
        } else if let first, first.isEmpty {
            Text("Search \(title) for artists, albums and songs.").foregroundStyle(.secondary).themedRows()
        } else if let first {
            ServedFoundRows(found: first)
        } else {
            ProgressView().frame(maxWidth: .infinity).themedRows()
        }
    }

    @ViewBuilder
    private func foundRows(scheme: String, title: String) -> some View {
        if let searchFailure {
            ReadFailure(text: searchFailure) { Task { await search(scheme) } }
                .themedRows()
        } else if let found, found.isEmpty {
            Text("\(title) found nothing for those words.").foregroundStyle(.secondary).themedRows()
        } else if let found {
            ServedFoundRows(found: found)
        } else {
            ProgressView().frame(maxWidth: .infinity).themedRows()
        }
    }

    private func loadFirst(_ service: String) async {
        failure = nil
        if firstOf != service {
            first = nil
            firstOf = service
        }
        guard !service.isEmpty else { return }
        do {
            let page = try await library.features.serverAnswer { try $0.servedFirstPage(service: service) }
            guard service == firstOf else { return }
            first = page
        } catch {
            if Task.isCancelled { return }
            failure = explained(error)
        }
    }

    private func search(_ service: String) async {
        let words = asked
        found = nil
        searchFailure = nil
        guard !words.isEmpty, !service.isEmpty else { return }
        do {
            let answer = try await library.features.serverAnswer { try $0.servedSearch(service: service, words: words) }
            guard words == asked else { return }
            found = answer
        } catch {
            if Task.isCancelled { return }
            searchFailure = explained(error)
        }
    }
}

struct ServedPlaceView: View {
    let place: ServedPlace

    var body: some View {
        switch place.kind {
        case .artist: ServedArtistPage(place: place)
        case .album: ServedAlbumPage(place: place)
        }
    }
}

struct ServedHeader: View {
    private static let largestArtwork: CGFloat = 300
    private static let widestActions: CGFloat = 440

    @ScaledMetric(relativeTo: .title2) private var artworkEdge: CGFloat = 220
    let place: ServedPlace
    let summary: String
    let playable: Bool
    let play: () -> Void
    let shuffle: () -> Void
    @State private var acted = 0

    var body: some View {
        let edge = min(artworkEdge, Self.largestArtwork)
        let circle = place.kind == .artist
        VStack(spacing: 16) {
            ServedCoverView(uri: place.uri, cover: place.cover, edge: edge, circle: circle, corner: CollectionCover.corner)
                .background { HeaderShadow(outline: circle ? .circle : .rounded(CollectionCover.corner)) }
            VStack(spacing: 6) {
                Text(place.name)
                    .font(.title2.weight(.bold))
                    .multilineTextAlignment(.center)
                    .accessibilityAddTraits(.isHeader)
                Text(circle ? "Artist" : place.artist)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                Text(summary).font(.footnote).foregroundStyle(.secondary)
            }
            HStack(spacing: 12) {
                CollectionAction(title: "Play", symbol: "play.fill", prominent: true) {
                    acted += 1
                    play()
                }
                CollectionAction(title: "Shuffle", symbol: "shuffle", prominent: false) {
                    acted += 1
                    shuffle()
                }
            }
            .frame(maxWidth: Self.widestActions)
            .disabled(!playable)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 12)
        .sensoryFeedback(.success, trigger: acted)
        .listRowSeparator(.hidden)
        .listRowBackground(Color.clear)
    }
}

struct ServedArtistPage: View {
    @Environment(Library.self) private var library
    @ScaledMetric(relativeTo: .subheadline) private var albumEdge: CGFloat = 140
    let place: ServedPlace
    @State private var albums: [ServedAlbum] = []
    @State private var read = false
    @State private var failure: String?

    var body: some View {
        List {
            ServedHeader(
                place: place,
                summary: counted(albums.count, "album"),
                playable: !albums.isEmpty,
                play: { library.playServed(expanding: place.uri, shuffled: false) },
                shuffle: { library.playServed(expanding: place.uri, shuffled: true) }
            )
            if let failure {
                ReadFailure(text: failure) { Task { await load() } }
            } else if read, albums.isEmpty {
                Text("No albums are listed for this artist.").foregroundStyle(.secondary)
            }
            if !albums.isEmpty {
                Section("Albums") {
                    ScrollView(.horizontal, showsIndicators: false) {
                        LazyHStack(spacing: 14) {
                            ForEach(albums, id: \.uri) { album in
                                NavigationLink(value: Destination.served(ServedPlace(album))) {
                                    VStack(alignment: .leading) {
                                        ServedCoverView(uri: album.uri, cover: album.cover, edge: albumEdge, corner: 10)
                                        Text(album.shownName)
                                            .font(.subheadline.weight(.medium))
                                            .lineLimit(1)
                                        Text(album.year.map(String.init) ?? album.songs.map { counted(Int($0), "song") } ?? "")
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                    }
                                    .frame(width: albumEdge)
                                    .accessibilityElement(children: .combine)
                                }
                                .buttonStyle(.plain)
                                .servedActions(album.uri)
                            }
                        }
                        .scrollTargetLayout()
                    }
                    .scrollTargetBehavior(.viewAligned)
                    .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))
                }
                .listRowBackground(Color.clear)
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .navigationTitle(place.name)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar { ServedMenu(uri: place.uri) }
        .task(id: library.features.servedGeneration) { await load() }
    }

    private func load() async {
        let uri = place.uri
        failure = nil
        do {
            albums = try await library.features.serverAnswer { try $0.servedAlbums(artist: uri) }
            read = true
        } catch {
            if Task.isCancelled { return }
            failure = explained(error)
        }
    }
}

struct ServedAlbumPage: View {
    @Environment(Library.self) private var library
    let place: ServedPlace
    @State private var songs: [ServedSong] = []
    @State private var read = false
    @State private var failure: String?

    var body: some View {
        List {
            ServedHeader(
                place: place,
                summary: servedSummary(of: songs),
                playable: !songs.isEmpty,
                play: { library.playServed(songs.map(\.uri), at: 0) },
                shuffle: { library.shuffleServed(songs.map(\.uri)) }
            )
            if let failure {
                ReadFailure(text: failure) { Task { await load() } }
            } else if read, songs.isEmpty {
                Text("No songs are listed for this album.").foregroundStyle(.secondary)
            }
            let uris = songs.map(\.uri)
            ForEach(Array(songs.enumerated()), id: \.element.uri) { index, song in
                ServedSongRow(song: song, number: song.number(at: index)) {
                    library.playServed(uris, at: index, shuffle: library.snapshot?.shuffle == true)
                }
                .listRowBackground(Color.clear)
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .navigationTitle(place.name)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar { ServedMenu(uri: place.uri) }
        .task(id: library.features.servedGeneration) { await load() }
    }

    private func load() async {
        let uri = place.uri
        failure = nil
        do {
            songs = try await library.features.serverAnswer { try $0.servedSongs(album: uri) }
            read = true
        } catch {
            if Task.isCancelled { return }
            failure = explained(error)
        }
    }
}

struct ServedMenu: ToolbarContent {
    @Environment(Library.self) private var library
    let uri: String

    var body: some ToolbarContent {
        ToolbarItem(placement: .topBarTrailing) {
            Menu("More", systemImage: "ellipsis") {
                Button("Play next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                    library.playServedNext(expanding: uri)
                }
                Button("Add to queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                    library.addServedToQueue(expanding: uri)
                }
            }
        }
    }
}
