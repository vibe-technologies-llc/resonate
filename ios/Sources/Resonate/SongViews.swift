import ResonateNative
import SwiftUI

extension Track {
    var shownTitle: String { title.isEmpty ? name : title }
    var shownArtist: String { artist.isEmpty ? "Unknown artist" : artist }
    var groupingArtist: String { albumArtist.isEmpty ? artist : albumArtist }

    var quality: String {
        var parts = [codec]
        if let bits { parts.append("\(bits)-bit") }
        if rate > 0 { parts.append(String(format: "%g kHz", Double(rate) / 1000)) }
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

enum Grouping: String, CaseIterable, Identifiable {
    case songs = "Songs"
    case albums = "Albums"
    case artists = "Artists"

    var id: String { rawValue }
}

struct SongsView: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @State private var grouping = Grouping.songs

    var body: some View {
        @Bindable var library = library
        List {
            Section {
                Picker("Show", selection: $grouping) {
                    ForEach(Grouping.allCases) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack {
                        Button("Playlists") { navigation.open(.playlists) }
                        Button("Suggestions") { navigation.open(.suggestions) }
                        Button("Online search") { navigation.open(.online) }
                        Button("Filters") { navigation.filters = true }
                    }
                    .buttonStyle(.glass)
                }
                .listRowInsets(EdgeInsets(top: 4, leading: 16, bottom: 4, trailing: 16))
            }
            .listRowSeparator(.hidden)
            if library.tracks.isEmpty, !library.scanning, library.search.isEmpty {
                EmptyLibrary()
            } else {
                switch grouping {
                case .songs:
                    if library.tracks.isEmpty {
                        Text("No songs match").foregroundStyle(.secondary)
                    }
                    ForEach(library.tracks, id: \.id) { track in
                        TrackRow(track: track, current: library.snapshot?.current?.id == track.id) {
                            library.play(library.tracks, selected: track)
                        }
                    }
                case .albums:
                    ForEach(albums(of: library.tracks), id: \.key) { group in
                        NavigationLink(value: Destination.album(title: group.title, artist: group.artist)) {
                            CollectionRow(track: group.tracks.first, title: group.title.isEmpty ? "Unknown album" : group.title, detail: "\(group.artist) · \(counted(group.tracks.count, "song"))")
                        }
                    }
                case .artists:
                    ForEach(artists(of: library.tracks), id: \.key) { group in
                        NavigationLink(value: Destination.artist(group.title)) {
                            CollectionRow(track: group.tracks.first, title: group.title.isEmpty ? "Unknown artist" : group.title, detail: "Artist · \(counted(group.tracks.count, "song"))", circle: true)
                        }
                    }
                }
            }
        }
        .listStyle(.plain)
        .searchable(text: $library.search, prompt: "Search your music")
        .navigationTitle("Resonate")
        .toolbar { LibraryToolbar() }
    }
}

struct Collected: Identifiable {
    let key: String
    let title: String
    let artist: String
    let tracks: [Track]

    var id: String { key }
}

func albums(of tracks: [Track]) -> [Collected] {
    var order: [String] = []
    var grouped: [String: Collected] = [:]
    for track in tracks {
        let key = "\(track.album.count):\(track.album)\(track.groupingArtist)"
        if let group = grouped[key] {
            grouped[key] = Collected(key: key, title: group.title, artist: group.artist, tracks: group.tracks + [track])
        } else {
            order.append(key)
            grouped[key] = Collected(key: key, title: track.album, artist: track.groupingArtist, tracks: [track])
        }
    }
    return order.compactMap { grouped[$0] }
}

func artists(of tracks: [Track]) -> [Collected] {
    var order: [String] = []
    var grouped: [String: [Track]] = [:]
    for track in tracks {
        if grouped[track.artist] == nil { order.append(track.artist) }
        grouped[track.artist, default: []].append(track)
    }
    return order.map { Collected(key: $0, title: $0, artist: $0, tracks: grouped[$0] ?? []) }
}

struct EmptyLibrary: View {
    @Environment(Navigation.self) private var navigation

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "music.note.house")
                .font(.system(size: 56))
                .foregroundStyle(.tint)
            Text("Your music, in its own detail").font(.title2.weight(.semibold))
            Text("Choose a music folder to build your library, or put songs in Resonate's folder in the Files app. Your files stay where they are.")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
            Button("Choose music folder") { navigation.choosingFolder = true }
                .buttonStyle(.glassProminent)
        }
        .frame(maxWidth: .infinity)
        .padding(32)
        .listRowSeparator(.hidden)
    }
}

struct CollectionRow: View {
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 56
    let track: Track?
    let title: String
    let detail: String
    var circle = false

    var body: some View {
        HStack(spacing: 14) {
            ArtworkView(track: track, corner: 10, circle: circle)
                .frame(width: artworkEdge, height: artworkEdge)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.body.weight(.medium)).lineLimit(1)
                Text(detail).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

struct TrackRow: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 48
    @ScaledMetric(relativeTo: .callout) private var numberWidth: CGFloat = 28
    let track: Track
    var current: Bool?
    var numbered: UInt32?
    var ownsMenus = true
    let play: () -> Void
    @State private var choosingPlaylist = false
    @State private var queued = 0

    var body: some View {
        let playingHere = current ?? (library.currentId == track.id)
        HStack(spacing: 12) {
            Button(action: play) {
                HStack(spacing: 12) {
                    leading(playingHere)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(track.shownTitle)
                            .font(.body.weight(playingHere ? .semibold : .regular))
                            .foregroundStyle(playingHere ? Color.accentColor : Color.primary)
                            .lineLimit(1)
                        Text("\(track.shownArtist) · \(clock(track.durationMs))")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                    Spacer(minLength: 0)
                }
                .contentShape(.rect)
            }
            .buttonStyle(.plain)
            .accessibilityValue(playingHere ? "Now playing" : "")
            FavouriteButton(track: track)
        }
        .swipeActions(edge: .leading, allowsFullSwipe: true) {
            Button("Play next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                queued += 1
                library.playNext([track])
            }
            .tint(.indigo)
            Button("Add to queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                queued += 1
                library.addToQueue([track])
            }
            .tint(.orange)
        }
        .modifier(StandardTrackMenus(track: track, enabled: ownsMenus, choosingPlaylist: $choosingPlaylist, queued: $queued))
        .sensoryFeedback(.success, trigger: queued)
        .sheet(isPresented: $choosingPlaylist) { PlaylistPicker(tracks: [track]) }
        .accessibilityAction(named: "Song details") { navigation.open(.inspector(track)) }
    }

    @ViewBuilder
    private func leading(_ playingHere: Bool) -> some View {
        if let numbered {
            ZStack {
                if playingHere {
                    NowPlayingIndicator()
                } else {
                    Text("\(numbered)").font(.callout.monospacedDigit()).foregroundStyle(.secondary)
                }
            }
            .frame(width: numberWidth)
        } else {
            ArtworkView(track: track, corner: 8)
                .frame(width: artworkEdge, height: artworkEdge)
                .overlay {
                    if playingHere {
                        NowPlayingIndicator()
                            .foregroundStyle(.white)
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                            .background(.black.opacity(0.35), in: .rect(cornerRadius: 8))
                    }
                }
        }
    }
}

struct StandardTrackMenus: ViewModifier {
    @Environment(Library.self) private var library
    let track: Track
    let enabled: Bool
    @Binding var choosingPlaylist: Bool
    @Binding var queued: Int

    func body(content: Content) -> some View {
        if enabled {
            let favourite = library.isFavourite(track)
            content
                .swipeActions(edge: .trailing) {
                    Button(favourite ? "Remove from favourites" : "Add to favourites", systemImage: favourite ? "heart.slash" : "heart") {
                        library.setFavourite(track, !favourite)
                    }
                    .tint(.pink)
                    Button("Add to playlist", systemImage: "text.badge.plus") { choosingPlaylist = true }
                        .tint(.blue)
                }
                .contextMenu { TrackMenu(track: track, choosingPlaylist: $choosingPlaylist, queued: $queued) }
        } else {
            content
        }
    }
}

struct NowPlayingIndicator: View {
    @Environment(Library.self) private var library

    var body: some View {
        Image(systemName: "waveform")
            .font(.callout.weight(.semibold))
            .foregroundStyle(.tint)
            .symbolEffect(.variableColor.iterative.dimInactiveLayers, isActive: library.transport == .playing)
            .accessibilityHidden(true)
    }
}

struct TrackMenu: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let track: Track
    @Binding var choosingPlaylist: Bool
    var queued: Binding<Int>?

    var body: some View {
        let favourite = library.isFavourite(track)
        Section {
            Button("Play next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                queued?.wrappedValue += 1
                library.playNext([track])
            }
            Button("Add to queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                queued?.wrappedValue += 1
                library.addToQueue([track])
            }
        }
        Section {
            Button(favourite ? "Remove from favourites" : "Add to favourites", systemImage: favourite ? "heart.slash" : "heart") {
                library.setFavourite(track, !favourite)
            }
            Button("Add to playlist", systemImage: "text.badge.plus") { choosingPlaylist = true }
        }
        Section {
            Button("Song details", systemImage: "info.circle") { navigation.open(.inspector(track)) }
            Button("Lyrics", systemImage: "quote.bubble") { navigation.open(.lyrics(track)) }
            Button("Analyse", systemImage: "waveform.path.ecg") { navigation.open(.analysis(track)) }
            if !track.album.isEmpty {
                Button("Go to album", systemImage: "square.stack") {
                    navigation.open(.album(title: track.album, artist: track.groupingArtist))
                }
            }
            if !track.artist.isEmpty {
                Button("Go to artist", systemImage: "music.mic") { navigation.open(.artist(track.artist)) }
            }
        }
    }
}
