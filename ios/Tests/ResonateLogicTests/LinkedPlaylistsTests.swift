import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct LinkedPlaylistsTests {
    @Test func theHeaderSaysWhetherThePlaylistFollowsItsLink() {
        let kept = PlaylistLinkInfo(page: "https://open.spotify.com/playlist/1", service: "Spotify", keptInStep: true, refreshed: 0)
        var once = kept
        once.keptInStep = false

        #expect(LinkedPlaylistWords.header(kept) == "Kept in step with Spotify")
        #expect(LinkedPlaylistWords.header(once) == "Imported from Spotify")
    }

    @Test func anImportSaysWhatIsHereAndWhatIsComing() {
        #expect(
            LinkedPlaylistWords.imported(name: "Night drive", service: "Spotify", keptInStep: false, matched: 3, missing: 2)
                == "Imported 3 local songs; 2 still missing")
        #expect(
            LinkedPlaylistWords.imported(name: "Night drive", service: "Spotify", keptInStep: true, matched: 1, missing: 4)
                == "Keeping Night drive in step with Spotify · 1 song here, 4 being downloaded")
        #expect(
            LinkedPlaylistWords.imported(name: "Night drive", service: "Deezer", keptInStep: true, matched: 5, missing: 0)
                == "Keeping Night drive in step with Deezer · 5 songs here")
    }

    @Test func aRefreshAskedAlwaysAnswersAndAQuietOneOnlyWhenSongsMoved() {
        let still = RefreshedPlaylist(id: 1, added: 0, removed: 0, missing: 2, wanted: 2)
        let moved = RefreshedPlaylist(id: 1, added: 2, removed: 1, missing: 3, wanted: 3)
        let gained = RefreshedPlaylist(id: 1, added: 1, removed: 0, missing: 4, wanted: 0)

        #expect(!LinkedPlaylistWords.moved(still))
        #expect(LinkedPlaylistWords.moved(moved))
        #expect(LinkedPlaylistWords.refreshed(name: "Mix", service: "Spotify", still, asked: false) == nil)
        #expect(LinkedPlaylistWords.refreshed(name: "Mix", service: "Spotify", still, asked: true) == "Mix from Spotify · 2 being downloaded")
        #expect(
            LinkedPlaylistWords.refreshed(name: "Mix", service: "Spotify", RefreshedPlaylist(id: 1, added: 0, removed: 0, missing: 0, wanted: 0), asked: true)
                == "Mix is in step with Spotify")
        #expect(
            LinkedPlaylistWords.refreshed(name: "Mix", service: "Spotify", moved, asked: true)
                == "Mix from Spotify · 2 songs added, 1 taken out, 3 being downloaded")
        #expect(
            LinkedPlaylistWords.refreshed(name: "Mix", service: "Spotify", moved, asked: false)
                == "Mix from Spotify · 2 songs added, 1 taken out")
        #expect(
            LinkedPlaylistWords.refreshed(name: "Mix", service: "TIDAL", gained, asked: true)
                == "Mix from TIDAL · 1 song added, 4 not in your library")
    }

    @Test func theTimedRefreshLooksAMinuteInThenEveryTenWhileActiveAndOnline() {
        #expect(LinkedPlaylistPace.firstLook == .seconds(60))
        #expect(LinkedPlaylistPace.lookEvery == .seconds(600))
        #expect(LinkedPlaylistPace.follows(online: true, foreground: true))
        #expect(!LinkedPlaylistPace.follows(online: false, foreground: true))
        #expect(!LinkedPlaylistPace.follows(online: true, foreground: false))
    }
}

@Suite struct FollowedProfileWordsTests {
    @Test func aFollowedProfileIsNamedByItsNameAndServiceNotItsLink() {
        let followed = ProfileFollowed(link: "https://www.deezer.com/profile/2529", name: "dadbond", service: "Deezer")

        #expect(FollowedProfileWords.followed(followed) == "dadbond on Deezer")
        #expect(FollowedProfileWords.stopFollowing(followed.name) == "Stop following dadbond?")
    }

    private func profile(_ playlists: Int, letGo: UInt32 = 0, gone: UInt32 = 0, unread: UInt32 = 0) -> FollowedProfile {
        FollowedProfile(
            name: "Ada",
            service: "Deezer",
            playlists: (0..<playlists).map { ImportedPlaylist(id: UInt64($0), matched: 1, missing: 0) },
            letGo: letGo,
            gone: gone,
            unread: unread
        )
    }

    @Test func followingSaysHowManyPlaylistsAndOnlyTheCountsThatHappened() {
        #expect(
            FollowedProfileWords.followed(profile(3), keptInStep: true) == "Following Ada on Deezer · 3 playlists")
        #expect(
            FollowedProfileWords.followed(profile(1, letGo: 2, gone: 1, unread: 4), keptInStep: true)
                == "Following Ada on Deezer · 1 playlist, 2 let go, 1 deleted on Deezer, 4 could not be read")
    }

    @Test func movingOnceSaysWhatMovedAndNothingOfFollowing() {
        #expect(FollowedProfileWords.followed(profile(2), keptInStep: false) == "Moved 2 playlists of Ada to Resonate")
        #expect(
            FollowedProfileWords.followed(profile(2, letGo: 1, gone: 1, unread: 1), keptInStep: false)
                == "Moved 2 playlists of Ada to Resonate, 1 could not be read")
    }

    @Test func aQuietRefreshSpeaksOnlyWhenSomethingMoved() {
        #expect(FollowedProfileWords.refreshed(name: "Ada", service: "Deezer", added: 0, changed: false, gone: 0) == nil)
        #expect(
            FollowedProfileWords.refreshed(name: "Ada", service: "Deezer", added: 0, changed: true, gone: 0)
                == "Ada's playlists on Deezer brought in step")
        #expect(
            FollowedProfileWords.refreshed(name: "Ada", service: "Deezer", added: 2, changed: true, gone: 1)
                == "Ada on Deezer · 2 new playlists, 1 deleted on Deezer")
    }

    @Test func theOffersNameTheService() {
        #expect(FollowedProfileWords.follow("Deezer") == "Follow Deezer profile")
        #expect(FollowedProfileWords.moveOnce == "Move to Resonate once")
    }
}
