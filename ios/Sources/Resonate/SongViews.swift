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
    let track: Track?
    let title: String
    let detail: String
    var circle = false

    var body: some View {
        HStack(spacing: 14) {
            ArtworkView(track: track, corner: 10, circle: circle)
                .frame(width: 56, height: 56)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.body.weight(.medium)).lineLimit(1)
                Text(detail).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
            }
        }
    }
}

struct TrackRow: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let track: Track
    var current = false
    var numbered: UInt32?
    let play: () -> Void
    @State private var choosingPlaylist = false

    var body: some View {
        HStack(spacing: 12) {
            Button(action: play) {
                HStack(spacing: 12) {
                    if let numbered {
                        Text("\(numbered)").font(.callout.monospacedDigit()).foregroundStyle(.secondary).frame(width: 28)
                    } else {
                        ArtworkView(track: track, corner: 8).frame(width: 48, height: 48)
                    }
                    VStack(alignment: .leading, spacing: 2) {
                        Text(track.shownTitle)
                            .font(.body.weight(current ? .semibold : .regular))
                            .foregroundStyle(current ? Color.accentColor : Color.primary)
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
            FavouriteButton(track: track)
        }
        .contextMenu { TrackMenu(track: track, choosingPlaylist: $choosingPlaylist) }
        .sheet(isPresented: $choosingPlaylist) { PlaylistPicker(tracks: [track]) }
        .accessibilityAction(named: "Song details") { navigation.open(.inspector(track)) }
    }
}

struct TrackMenu: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let track: Track
    @Binding var choosingPlaylist: Bool

    var body: some View {
        Button(track.favourite ? "Remove from favourites" : "Add to favourites", systemImage: track.favourite ? "heart.slash" : "heart") {
            library.setFavourite(track, !track.favourite)
        }
        Button("Add to playlist", systemImage: "text.badge.plus") { choosingPlaylist = true }
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
