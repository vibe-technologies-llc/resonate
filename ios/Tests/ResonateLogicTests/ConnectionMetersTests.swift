import Testing

@testable import ResonateLogic

@Suite struct ConnectionMetersTests {
    @Test func aSatisfiedPathThatIsNeitherExpensiveNorConstrainedIsUnmetered() {
        #expect(ConnectionMeters.of(satisfied: true, expensive: false, constrained: false) == .unmetered)
    }

    @Test func anExpensiveOrConstrainedPathIsMetered() {
        #expect(ConnectionMeters.of(satisfied: true, expensive: true, constrained: false) == .metered)
        #expect(ConnectionMeters.of(satisfied: true, expensive: false, constrained: true) == .metered)
        #expect(ConnectionMeters.of(satisfied: true, expensive: true, constrained: true) == .metered)
    }

    @Test func anUnsatisfiedPathIsUnknownWhateverItsFlags() {
        #expect(ConnectionMeters.of(satisfied: false, expensive: false, constrained: false) == .unknown)
        #expect(ConnectionMeters.of(satisfied: false, expensive: true, constrained: true) == .unknown)
    }

    @Test func theWaitingLineShowsOnlyWhileDownloadsWaitOnTheMeter() {
        #expect(ConnectionMeters.line(waiting: true) == "Waiting for a network that is not metered")
        #expect(ConnectionMeters.line(waiting: false) == nil)
    }
}
