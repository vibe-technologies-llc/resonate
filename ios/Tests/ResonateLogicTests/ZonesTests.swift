import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct ZonesTests {
    @Test func theZonesAreReadAgainWhenTheyOrTheZoneShownMove() {
        let read = ZonesListed(snapshot(zone: 1, zonesRevision: 3))

        #expect(ZonesListed(snapshot()).isDue(after: nil, zones: 0, ticks: 1))
        #expect(!ZonesListed(snapshot(zone: 1, zonesRevision: 3)).isDue(after: read, zones: 1, ticks: 50))
        #expect(ZonesListed(snapshot(zone: 2, zonesRevision: 3)).isDue(after: read, zones: 1, ticks: 1))
        #expect(ZonesListed(snapshot(zone: 1, zonesRevision: 4)).isDue(after: read, zones: 1, ticks: 1))
    }

    @Test func severalZonesAreReadNowAndThenSoEachSaysWhatItPlays() {
        let read = ZonesListed(snapshot(zone: 1, zonesRevision: 3))
        let same = ZonesListed(snapshot(zone: 1, zonesRevision: 3))

        #expect(!same.isDue(after: read, zones: 2, ticks: ZonesListed.readEveryTicks - 1))
        #expect(same.isDue(after: read, zones: 2, ticks: ZonesListed.readEveryTicks))
    }

    @Test func theSongIsHandedOnlyToAnotherZoneAndTheFirstZoneStays() {
        #expect(ZonePlan.mayHandOver(to: 2, shown: 1, shownHolds: true))
        #expect(!ZonePlan.mayHandOver(to: 1, shown: 1, shownHolds: true))
        #expect(!ZonePlan.mayHandOver(to: 2, shown: 1, shownHolds: false))
        #expect(!ZonePlan.mayRemove(ZonePlan.first))
        #expect(ZonePlan.mayRemove(2))
    }

    @Test func aZoneSaysWhatItIsDoing() {
        #expect(ZonePlan.line(.playing, playing: "Echoes") == "Playing Echoes")
        #expect(ZonePlan.line(.paused, playing: nil) == "Paused")
        #expect(ZonePlan.line(.stopped, playing: "Echoes") == "Stopped")
    }
}
