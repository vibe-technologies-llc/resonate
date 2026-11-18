import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct ListeningHistoryTests {
    @Test func theHistoryIsKeptForeverUntilASpanIsSavedAndForeverTakesTheSpanAway() {
        let settings = Defaults().first

        let unsaid = ListeningHistory.kept(in: settings)
        ListeningHistory.keep(.days(days: 365), in: settings)
        let aYear = ListeningHistory.kept(in: settings)
        ListeningHistory.keep(.forever, in: settings)

        #expect(unsaid == .forever)
        #expect(aYear == .days(days: 365))
        #expect(ListeningHistory.kept(in: settings) == .forever)
        #expect(settings.object(forKey: "history-kept-days") == nil)
    }

    @Test func onlyAShorterSpanForgetsListens() {
        #expect(ListeningHistory.forgetsListens(keeping: .forever, asked: .days(days: 365)))
        #expect(ListeningHistory.forgetsListens(keeping: .days(days: 365), asked: .days(days: 182)))
        #expect(!ListeningHistory.forgetsListens(keeping: .days(days: 182), asked: .days(days: 365)))
        #expect(!ListeningHistory.forgetsListens(keeping: .days(days: 365), asked: .forever))
        #expect(!ListeningHistory.forgetsListens(keeping: .forever, asked: .forever))
    }

    @Test func eachSpanTheDesktopOffersIsNamedAsItNamesItAndAnyOtherByItsDays() {
        #expect(ListeningHistory.label(.forever) == "Forever")
        #expect(ListeningHistory.label(.days(days: 1826)) == "Five years")
        #expect(ListeningHistory.label(.days(days: 730)) == "Two years")
        #expect(ListeningHistory.label(.days(days: 365)) == "A year")
        #expect(ListeningHistory.label(.days(days: 182)) == "Six months")
        #expect(ListeningHistory.label(.days(days: 1)) == "1 day")
        #expect(ListeningHistory.label(.days(days: 90)) == "90 days")
    }

    @Test func anImportSaysWhatLandedWhatWasHereAndWhatNamesSongsNotHere() {
        #expect(
            ListeningHistory.broughtIn(HistoryBroughtIn(credited: 1, heldAlready: 2, unmatched: 3))
                == "Brought in 1 listen · 2 already here · 3 of songs not on this phone"
        )
    }
}
