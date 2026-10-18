import AppIntents
import ResonateNative

struct ResumePlaybackIntent: AudioPlaybackIntent {
    static let title: LocalizedStringResource = "Play"
    static let description = IntentDescription("Carries on with what was playing, or shuffles your library when nothing was.")

    @Dependency private var library: Library

    init() {}

    @MainActor
    func perform() async throws -> some IntentResult & ProvidesDialog {
        let native = try await Spoken.player(of: library)
        let current = try await Work.inOrder { try native.snapshot().current }

        guard let current else {
            return .result(dialog: try await Spoken.shuffleEverything(in: library, from: native))
        }
        library.resume()
        return .result(dialog: "Resuming \(current.title).")
    }
}

struct PlayFromLibraryIntent: AudioPlaybackIntent {
    static let title: LocalizedStringResource = "Play from Your Library"
    static let description = IntentDescription("Plays a playlist, artist, album or songs from your library that match what you ask for.")

    @Parameter(title: "Music", requestValueDialog: "What would you like to play?")
    var music: String

    @Dependency private var library: Library

    init() {}

    @MainActor
    func perform() async throws -> some IntentResult & ProvidesDialog {
        let native = try await Spoken.player(of: library)
        let asked = music.trimmingCharacters(in: .whitespacesAndNewlines)
        let wanted = try await Work.aside { try Wanted.matching(asked, from: native) }

        guard !wanted.tracks.isEmpty else {
            return .result(dialog: "Nothing in your library matches “\(asked)”.")
        }
        library.play(wanted.tracks, at: 0)
        return .result(dialog: "\(wanted.announced)")
    }
}

struct ShuffleLibraryIntent: AudioPlaybackIntent {
    static let title: LocalizedStringResource = "Shuffle Your Library"
    static let description = IntentDescription("Shuffles every song in your library.")

    @Dependency private var library: Library

    init() {}

    @MainActor
    func perform() async throws -> some IntentResult & ProvidesDialog {
        let native = try await Spoken.player(of: library)
        return .result(dialog: try await Spoken.shuffleEverything(in: library, from: native))
    }
}

struct PlayFavouritesIntent: AudioPlaybackIntent {
    static let title: LocalizedStringResource = "Play Favourites"
    static let description = IntentDescription("Plays the songs you marked as favourites.")

    @Dependency private var library: Library

    init() {}

    @MainActor
    func perform() async throws -> some IntentResult & ProvidesDialog {
        let native = try await Spoken.player(of: library)
        var favourites = defaultTrackFilter()
        favourites.favourites = true
        let chosen = favourites
        let tracks = try await Work.aside { try Library.everyPage { try native.filteredTracks(filter: chosen, offset: $0, limit: Library.page) } }

        guard !tracks.isEmpty else {
            return .result(dialog: "You have no favourite songs yet.")
        }
        library.play(tracks, at: 0)
        return .result(dialog: "Playing your favourites.")
    }
}

struct ResonateShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: ResumePlaybackIntent(),
            phrases: [
                "Play \(.applicationName)",
                "Resume \(.applicationName)",
                "Play music in \(.applicationName)",
                "Play music with \(.applicationName)",
            ],
            shortTitle: "Play",
            systemImageName: "play.fill"
        )
        AppShortcut(
            intent: PlayFromLibraryIntent(),
            phrases: [
                "Play something in \(.applicationName)",
                "Play a song in \(.applicationName)",
                "Play an album in \(.applicationName)",
                "Play an artist in \(.applicationName)",
                "Play a playlist in \(.applicationName)",
                "Find music in \(.applicationName)",
            ],
            shortTitle: "Play from Library",
            systemImageName: "magnifyingglass"
        )
        AppShortcut(
            intent: ShuffleLibraryIntent(),
            phrases: [
                "Shuffle \(.applicationName)",
                "Shuffle my music in \(.applicationName)",
                "Shuffle my library in \(.applicationName)",
            ],
            shortTitle: "Shuffle All",
            systemImageName: "shuffle"
        )
        AppShortcut(
            intent: PlayFavouritesIntent(),
            phrases: [
                "Play my favourites in \(.applicationName)",
                "Play my favorites in \(.applicationName)",
                "Play favourites in \(.applicationName)",
            ],
            shortTitle: "Favourites",
            systemImageName: "heart.fill"
        )
    }
}

@MainActor
enum Spoken {
    static func player(of library: Library) async throws -> MobilePlayer {
        library.start()
        library.openAgain()
        return try await library.player()
    }

    static func shuffleEverything(in library: Library, from native: MobilePlayer) async throws -> IntentDialog {
        let everything = defaultTrackFilter()
        let tracks = try await Work.aside { try Library.everyPage { try native.filteredTracks(filter: everything, offset: $0, limit: Library.page) } }

        guard !tracks.isEmpty else { return "Your library has no songs yet." }
        library.shuffle(tracks)
        return "Shuffling your library."
    }
}

struct Wanted: Sendable {
    let tracks: [Track]
    let announced: String

    nonisolated static func matching(_ asked: String, from native: MobilePlayer) throws -> Wanted {
        let folded = asked.searchFolded
        guard !folded.isEmpty else { return Wanted(tracks: [], announced: "") }

        if let playlist = try native.playlists().first(where: { $0.name.searchFolded == folded }) {
            let tracks = try Library.everyPage { try native.playlistTracks(id: playlist.id, offset: $0, limit: Library.page) }
            return Wanted(tracks: tracks, announced: "Playing your playlist \(playlist.name).")
        }

        let found = try native.searchLibrary(text: asked, limit: SearchModel.mostSongs)
        if let artist = found.artists.first(where: { $0.name.searchFolded == folded }) {
            return Wanted(tracks: try native.artistTracks(name: artist.name), announced: "Playing \(artist.shownName).")
        }
        if let album = found.albums.first(where: { $0.title.searchFolded == folded }) {
            return Wanted(tracks: try native.albumTracks(key: album.key), announced: "Playing \(album.shownTitle) by \(album.shownArtist).")
        }
        guard let first = found.tracks.first else { return Wanted(tracks: [], announced: "") }
        return Wanted(tracks: found.tracks, announced: "Playing \(first.title).")
    }
}
