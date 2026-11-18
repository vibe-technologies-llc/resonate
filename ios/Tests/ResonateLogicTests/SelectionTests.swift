import Testing

@testable import ResonateLogic

@Suite struct SelectionTests {
    @Test func nothingIsChosenUntilSelectingBegins() {
        var selection = RowSelection<UInt64>()

        selection.toggle(1)

        #expect(!selection.selecting)
        #expect(selection.count == 0)
    }

    @Test func beginningWithARowChoosesItAndATapTogglesTheRest() {
        var selection = RowSelection<UInt64>()

        selection.begin(with: 1)
        selection.toggle(2)
        selection.toggle(1)

        #expect(selection.selecting)
        #expect(selection.chosen == [2])
        #expect(selection.contains(2))
        #expect(!selection.contains(1))
    }

    @Test func selectingStaysOnWithNothingChosenUntilCleared() {
        var selection = RowSelection<UInt64>()

        selection.begin()

        #expect(selection.selecting)
        #expect(selection.count == 0)

        selection.begin(with: 4)
        selection.toggle(4)

        #expect(selection.selecting)

        selection.clear()

        #expect(!selection.selecting)
        #expect(selection.count == 0)
    }

    @Test func selectAllChoosesEveryRowAndSaysSo() {
        var selection = RowSelection<UInt64>()

        selection.selectAll([1, 2, 3])

        #expect(selection.count == 3)
        #expect(selection.chosen == [1, 2, 3])
    }

    @Test func rowsThatLeftTheListAreDropped() {
        var selection = RowSelection<UInt64>()

        selection.selectAll([1, 2, 3])
        selection.retain([2, 3, 9])

        #expect(selection.chosen == [2, 3])
        #expect(selection.selecting)
    }

    @Test func whatWasPickedComesBackInTheListsOrder() {
        var selection = RowSelection<UInt64>()

        selection.begin(with: 3)
        selection.toggle(1)

        #expect(selection.picked([1, 2, 3, 4]) { $0 } == [1, 3])
    }
}
