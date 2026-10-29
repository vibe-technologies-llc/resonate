import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

private func servedTrack(_ uri: String = "subsonic:track/7", queueId: UInt64? = 3) -> Track {
    var made = track(0, queueId: queueId)
    made.uri = uri
    return made
}

private func song(
    _ title: String = "Sing",
    artist: String? = "Air",
    album: String? = "Moon Safari",
    track: UInt32? = nil,
    durationMs: UInt64? = 215_000
) -> ServedSong {
    ServedSong(uri: "subsonic:track/1", title: title, artist: artist, album: album, track: track, durationMs: durationMs, format: nil, cover: nil)
}

@Suite struct ServedTrackTests {
    @Test func aServersRowWithNoCatalogIdIsServedAndALibraryRowIsNot() {
        #expect(servedTrack().isServed)
        #expect(!track(0).isServed)
        #expect(!track(5).isServed)
        var elsewhere = track(0)
        elsewhere.uri = "resonate-folder://local/0.flac"
        #expect(!elsewhere.isServed)
    }

    @Test func everyServicePlayedInPlaceNamesItsRowsByItsScheme() {
        for uri in ["subsonic:track/7", "tidal:track/7", "resonate:track/7", "hifi-api:track/7", "monochrome:track/7"] {
            #expect(servedTrack(uri).isServed)
        }
        #expect(PlayedInPlace.scheme(of: "hifi-api:album/3") == "hifi-api")
        #expect(PlayedInPlace.scheme(of: "resonate-folder://local/0.flac") == nil)
        #expect(PlayedInPlace.scheme(of: "file:///a.flac") == nil)
        #expect(PlayedInPlace.scheme(of: "tidal") == nil)
    }

    @Test func aServedRowsPictureIsKeyedByItsAddressAndALibraryRowsByItsId() {
        #expect(servedTrack("subsonic:track/7").pictureKey == "subsonic:track/7")
        #expect(servedTrack("tidal:track/7").pictureKey == "tidal:track/7")
        #expect(servedTrack("subsonic:track/8").pictureKey != servedTrack("subsonic:track/7").pictureKey)
        #expect(track(12).pictureKey == "12")
    }

    @Test func aServedCoverIsNamedByAddressGenerationAndSize() {
        let first = ArtworkRequest(subject: .served("subsonic:album/1"), revision: 0, generation: 0, bucket: .row)
        let another = ArtworkRequest(subject: .served("subsonic:album/2"), revision: 0, generation: 0, bucket: .row)
        let moved = ArtworkRequest(subject: .served("subsonic:album/1"), revision: 0, generation: 1, bucket: .row)
        let larger = ArtworkRequest(subject: .served("subsonic:album/1"), revision: 0, generation: 0, bucket: .page)

        #expect(Set([first.name, another.name, moved.name, larger.name]).count == 4)
        #expect(first.songName != another.songName)
    }
}

@Suite struct ServedAccountTests {
    private func made(
        _ provider: MusicProvider,
        server: String = "",
        user: String = "",
        password: String = "",
        clientId: String = "",
        refreshToken: String = ""
    ) -> DownloadAccount {
        DownloadAccount(provider: provider, server: server, user: user, password: password, clientId: clientId, clientSecret: "", refreshToken: refreshToken)
    }

    private var everyKind: ProviderAccounts {
        ProviderAccounts(kept: [
            made(.subsonic, server: "https://a", user: "ann", password: "one"),
            made(.tidal, clientId: "client", refreshToken: "token"),
            made(.hifiApi),
            made(.monochrome, server: "https://mono"),
        ])
    }

    @Test func everyAccountOnAndReadyIsServedWhileOnline() {
        let served = everyKind.served(online: true, hosted: true)

        #expect(served.map(\.provider) == [.subsonic, .tidal, .hifiApi, .monochrome])
        #expect(served == everyKind.asked)
    }

    @Test func nothingIsServedWhileOffline() {
        #expect(everyKind.served(online: false, hosted: true).isEmpty)
    }

    @Test func aHostedServiceIsServedOnlyWhileHostedServicesAreAllowed() {
        let declined = everyKind.served(online: true, hosted: false)
        let keyed = ProviderAccounts(kept: [made(.hifiApi, password: "key")]).served(online: true, hosted: false)

        #expect(declined.map(\.provider) == [.subsonic, .tidal, .monochrome])
        #expect(keyed.map(\.provider) == [.hifiApi])
    }

    @Test func anAccountSwitchedOffOrIncompleteServesNothing() {
        let incomplete = ProviderAccounts(kept: [made(.subsonic, server: "https://a", user: "me"), made(.tidal, clientId: "client")])
        let off = ProviderAccounts(kept: [made(.subsonic, server: "https://a", user: "me", password: "pw")], switchedOff: [.subsonic, .monochrome])

        #expect(incomplete.served(online: true, hosted: false).isEmpty)
        #expect(off.served(online: true, hosted: true).map(\.provider) == [.hifiApi])
    }
}

@Suite struct ServedChoiceTests {
    private let subsonic = ServedService(scheme: "subsonic", title: "Subsonic")
    private let tidal = ServedService(scheme: "tidal", title: "TIDAL")
    private let monochrome = ServedService(scheme: "monochrome", title: "Monochrome")

    @Test func theFirstServiceIsChosenAndOneServiceOffersNoChoice() {
        let one = ServedChoice(services: [tidal])
        let none = ServedChoice()

        #expect(one.chosen == "tidal")
        #expect(one.service == tidal)
        #expect(!one.choosing)
        #expect(none.chosen == nil)
        #expect(none.service == nil)
    }

    @Test func aChoiceIsKeptWhileItsServiceStaysServed() {
        var choice = ServedChoice(services: [subsonic, tidal])

        choice.choose("tidal")
        choice.offer([monochrome, tidal])

        #expect(choice.choosing)
        #expect(choice.chosen == "tidal")
    }

    @Test func aChoiceWhoseServiceGoesFallsToTheFirst() {
        var choice = ServedChoice(services: [subsonic, tidal])

        choice.choose("tidal")
        choice.offer([monochrome, subsonic])

        #expect(choice.chosen == "monochrome")

        choice.offer([])

        #expect(choice.chosen == nil)
    }

    @Test func aServiceNotServedCannotBeChosen() {
        var choice = ServedChoice(services: [subsonic, tidal])

        choice.choose("monochrome")

        #expect(choice.chosen == "subsonic")
    }
}

@Suite struct ServedWordsTests {
    @Test func anArtistCountsItsAlbumsWhenTheServerSaysHowMany() {
        #expect(ServedArtist(uri: "subsonic:artist/1", name: "Air", albums: 1, cover: nil).line == "1 album")
        #expect(ServedArtist(uri: "subsonic:artist/1", name: "Air", albums: 3, cover: nil).line == "3 albums")
        #expect(ServedArtist(uri: "subsonic:artist/1", name: "Air", albums: nil, cover: nil).line == nil)
        #expect(ServedArtist(uri: "subsonic:artist/1", name: "", albums: nil, cover: nil).shownName == "Unknown artist")
    }

    @Test func anAlbumNamesItsArtistYearAndSongsAndSkipsWhatTheServerLeftOut() {
        let full = ServedAlbum(uri: "subsonic:album/1", name: "Moon Safari", artist: "Air", year: 1998, songs: 10, cover: nil)
        let bare = ServedAlbum(uri: "subsonic:album/2", name: "", artist: " ", year: nil, songs: nil, cover: nil)

        #expect(full.line == "Air · 1998 · 10 songs")
        #expect(bare.line == "")
        #expect(bare.shownName == "Unknown album")
    }

    @Test func aSongInAnAlbumDropsTheAlbumAndOneFoundElsewhereKeepsIt() {
        #expect(song().line(withAlbum: false) == "Air · 3:35")
        #expect(song().line(withAlbum: true) == "Air · Moon Safari · 3:35")
        #expect(song(artist: nil, album: nil, durationMs: nil).line(withAlbum: true) == "")
    }

    @Test func aSongIsNumberedByItsTrackElseByItsPlace() {
        #expect(song(track: 7).number(at: 0) == 7)
        #expect(song().number(at: 2) == 3)
    }

    @Test func anAlbumSummaryCountsSongsAndAddsTheLengthOnlyWhenKnown() {
        #expect(servedSummary(of: [song(), song()]) == "2 songs · 7 min")
        #expect(servedSummary(of: [song(durationMs: nil)]) == "1 song")
    }

    @Test func aSearchThatFoundNothingIsEmptyAndOneThatFoundSomethingSaysWhat() {
        let nothing = ServedFound(artists: [], albums: [], songs: [])
        let some = ServedFound(
            artists: [ServedArtist(uri: "subsonic:artist/1", name: "Air", albums: nil, cover: nil)],
            albums: [],
            songs: [song(), song()]
        )

        #expect(nothing.isEmpty)
        #expect(some.summary == "1 artist · 2 songs")
    }
}

@Suite struct ServedPlaceTests {
    @Test func aPlaceIsKeptAndReadBackWhole() throws {
        let album = ServedPlace(ServedAlbum(uri: "subsonic:album/1", name: "Moon Safari", artist: "Air", year: nil, songs: nil, cover: "al-1"))
        let artist = ServedPlace(ServedArtist(uri: "subsonic:artist/1", name: "", albums: nil, cover: nil))

        let kept = try JSONDecoder().decode([ServedPlace].self, from: JSONEncoder().encode([album, artist]))

        #expect(kept == [album, artist])
        #expect(kept[0].kind == .album)
        #expect(kept[0].artist == "Air")
        #expect(kept[1].name == "Unknown artist")
    }
}
