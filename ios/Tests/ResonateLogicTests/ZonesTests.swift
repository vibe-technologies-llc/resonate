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

    @Test func theVolumeIsReadAgainFromTheZoneShownOnlyWhenAnotherIsShown() {
        let first = { (shown: Bool) in zone(1, shown: shown, volume: 0.3) }
        let second = { (shown: Bool) in zone(2, shown: shown, volume: 0.8) }

        let switched = ZoneVolume.afterShowing([first(false), second(true)], over: [first(true), second(false)])
        let handedBack = ZoneVolume.afterShowing([first(true), second(false)], over: [first(false), second(true)])
        let unmoved = ZoneVolume.afterShowing([zone(1, shown: true, volume: 0.9), second(false)], over: [first(true), second(false)])
        let firstListed = ZoneVolume.afterShowing([first(true)], over: [])

        #expect(switched == 0.8)
        #expect(handedBack == 0.3)
        #expect(unmoved == nil)
        #expect(firstListed == nil)
    }

    @Test func theSongIsHandedOnlyToAnotherZoneAndTheFirstZoneStays() {
        #expect(ZonePlan.mayHandOver(to: 2, shown: 1, shownHolds: true))
        #expect(!ZonePlan.mayHandOver(to: 1, shown: 1, shownHolds: true))
        #expect(!ZonePlan.mayHandOver(to: 2, shown: 1, shownHolds: false))
        #expect(!ZonePlan.mayRemove(ZonePlan.first))
        #expect(ZonePlan.mayRemove(2))
    }

    @Test func aZoneSaysWhatItIsDoing() {
        #expect(ZonePlan.line(.playing, playing: "Slow Water") == "Playing Slow Water")
        #expect(ZonePlan.line(.paused, playing: nil) == "Paused")
        #expect(ZonePlan.line(.stopped, playing: "Slow Water") == "Stopped")
    }

    @Test func aCastReceiverIsNamedAsOneBesideItsHostAndAStreamerByItsHostAlone() {
        #expect(ZonePlan.rendererLine(host: "192.168.1.20", viaCast: false) == "192.168.1.20")
        #expect(ZonePlan.rendererLine(host: "192.168.1.30", viaCast: true) == "Google Cast · 192.168.1.30")
    }

    @Test func aZoneOnARendererNamesItBeforeWhatItIsDoing() {
        #expect(ZonePlan.line(.playing, playing: "Slow Water", on: "Living room streamer") == "Living room streamer · Playing Slow Water")
        #expect(ZonePlan.line(.paused, playing: nil, on: nil) == "Paused")
    }
}

private func zone(_ id: UInt32, shown: Bool, volume: Float) -> PlayerZone {
    PlayerZone(
        id: id, name: "", route: "", device: 0, shown: shown, volume: volume,
        transport: .paused, playing: nil, renderer: nil, rendererName: nil
    )
}
