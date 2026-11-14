import Foundation
import Testing

@testable import ResonateLogic

@Suite struct SettingsCatalogTests {
    func opened(_ query: String) -> [Destination] {
        SettingsSearch.matching(query).compactMap { match in
            if case .page(let destination) = match.place.opening { destination } else { nil }
        }
    }

    @Test func theRootListsTheGroupsInTheirOrderWithNoSubPage() {
        let shown = SettingsSearch.matching("")
        let groups = shown.map(\.place.group).reduce(into: [SettingsGroup]()) { if $0.last != $1 { $0.append($1) } }

        #expect(groups == [.sound, .library, .online, .devices, .appearance, .app])
        #expect(shown.allSatisfy { $0.place.parent == nil && $0.said == nil })
        #expect(
            shown.map(\.place.title).prefix(5) == ["Equaliser", "Sound processing", "Audio output", "Playback", "Sleep timer"]
        )
        #expect(shown.last?.place == .erase)
    }

    @Test func theRowsAreWordedAsAndroidWordsThem() {
        #expect(SettingsPlace.providers.title == "Download sources")
        #expect(SettingsPlace.online.title == "Online features")
        #expect(SettingsPlace.sync.title == "Device sync")
        #expect(SettingsPlace.folders.title == "Music folders")
    }

    @Test(arguments: [
        ("AutoEq", Destination.headphoneCorrection),
        ("room", .roomCorrection),
        ("ABX", .listeningTest),
        ("microphone", .microphoneMeasurement),
    ])
    func aWordOfAnEqualiserSubPageOpensThatPage(query: String, page: Destination) {
        #expect(opened(query).first == page)
    }

    @Test func aMatchInsideAPageSaysWhatMatched() {
        let match = SettingsSearch.matching("crossfeed").first

        #expect(match?.place == .processing)
        #expect(match?.said == "Crossfeed")
    }

    @Test func aSubPageFoundIsDetailedUnderItsPage() {
        let found = SettingsSearch.matching("SOFA").first
        let room = SettingsSearch.matching("room").first

        #expect(found?.detail(status: "unused") == "Equaliser · SOFA")
        #expect(room?.detail(status: "On · Studio") == "Equaliser · On · Studio")
        #expect(SettingsSearch.matching("crossfeed").first?.detail(status: "unused") == "Crossfeed")
    }

    @Test func theCorrectionsSayWhatAndroidSays() {
        #expect(SettingsPlace.headphoneCorrection.summary == "Search AutoEq or paste a profile")
        #expect(SettingsPlace.microphoneMeasurement.summary == "Fit a measurement or a sweep toward a target")
        #expect(SettingsPlace.listeningTest.summary == "A blind ABX test of the curve or a setting")
    }

    @Test func aMatchOnTheTitleSaysNothingMore() {
        #expect(SettingsSearch.matching("scrobbling").first?.said == nil)
    }

    @Test func everyWordMustBeginAWordOfThePageInAnyOrderCaseOrAccent() {
        #expect(opened("BUFFER playback").contains(.output))
        #expect(opened("éqUAL").first == .sound)
        #expect(opened("buffer lyrics").isEmpty)
    }

    @Test func aWordInsideAnotherWordDoesNotMatch() {
        #expect(!opened("brainz").contains(.scrobbling))
    }

    @Test func aHiddenPlaceIsNeitherListedNorFound() {
        #expect(!SettingsSearch.matching("", hiding: [.server]).map(\.place).contains(.server))
        #expect(SettingsSearch.matching("navidrome", hiding: [.server]).isEmpty)
    }

    @Test func eachPageOpensOneDestinationKeptInTheSettingsStack() {
        for place in SettingsPlace.allCases {
            if case .page(let destination) = place.opening {
                #expect(destination.belongsToSettings, "\(place)")
            }
        }
    }

    @Test func aSubPageIsShownUnderItsParentsGroup() {
        for place in SettingsPlace.allCases {
            if let parent = place.parent { #expect(parent.group == place.group) }
        }
    }

    @Test func searchWordsAreFoldedAndSplitButKeepTheirHyphens() {
        #expect(SettingsSearch.words("True-peak, Björk!") == ["true-peak", "bjork"])
        #expect(SettingsSearch.matching("truepeak").first?.place == .processing)
        #expect(SettingsSearch.matching("peak").map(\.place).contains(.processing))
    }
}

@Suite struct DestinationsTests {
    @Test func aStackKeptBeforeTheSettingsWereRebuiltStillOpensItsPages() throws {
        let kept = #"{"tab":"settings","paths":["settings",[{"sound":{}},{"processing":{}}],"library",[{"album":{"_0":"key"}},{"output":{}}],"home",[{"providers":{}}]]}"#
        let decoded = try #require(KeptNavigation.decoded(Data(kept.utf8)))

        #expect(decoded.tab == .settings)
        #expect(decoded.paths[.settings] == [.sound, .processing])
        #expect(decoded.paths[.library] == [.album("key"), .output])
        #expect(decoded.paths[.home] == [.providers])
    }

    @Test func theNewPagesAreKeptAndReadBack() throws {
        let kept = KeptNavigation(
            tab: .settings,
            paths: [.settings: [.sound, .roomCorrection], .home: [.sync, .syncDevice("abc")]]
        )
        let encoded = try #require(kept.encoded)

        #expect(KeptNavigation.decoded(encoded) == kept)
    }

    @Test func aStackHoldingAPageThisBuildDoesNotKnowIsNotRestored() {
        let kept = #"{"tab":"settings","paths":["settings",[{"somethingElse":{}}]]}"#

        #expect(KeptNavigation.decoded(Data(kept.utf8)) == nil)
    }

    @Test func theSettingsPagesBelongToTheSettingsTab() {
        #expect(Destination.sound.belongsToSettings)
        #expect(Destination.listeningTest.belongsToSettings)
        #expect(Destination.syncDevice("x").belongsToSettings)
        #expect(!Destination.album("x").belongsToSettings)
    }
}

@Suite struct EqualiserGraphShapeTests {
    @Test func theGraphIsSixteenByTenOnAPhone() {
        #expect(EqualiserGraphShape.height(forWidth: 320) == 200)
    }

    @Test func aNarrowGraphKeepsRoomForItsHandlesAndAWideOneStopsGrowing() {
        #expect(EqualiserGraphShape.height(forWidth: 100) == EqualiserGraphShape.shortest)
        #expect(EqualiserGraphShape.height(forWidth: 1_000) == EqualiserGraphShape.tallest)
    }
}
