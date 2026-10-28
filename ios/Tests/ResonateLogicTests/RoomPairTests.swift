import Testing

@testable import ResonateLogic

@Suite struct RoomPairTests {
    @Test func aPairIsNamedByBothFilesStemsLeftFirst() {
        #expect(RoomPair.name(left: "Studio L.wav", right: "Studio R.flac") == "Studio L and Studio R")
        #expect(RoomPair.name(left: "left.take.2.wav", right: "right") == "left.take.2 and right")
    }

    @Test func aFilesStemAndExtensionAreSplitAtItsLastDot() {
        #expect(RoomPair.stem(of: "room.v2.wav") == "room.v2")
        #expect(RoomPair.fileExtension(of: "room.v2.wav") == "wav")
        #expect(RoomPair.stem(of: "room") == "room")
        #expect(RoomPair.fileExtension(of: "room") == "")
    }
}
