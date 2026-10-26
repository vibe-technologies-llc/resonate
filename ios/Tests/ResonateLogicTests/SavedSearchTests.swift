import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct SavedSearchDraftTests {
    @Test func aBlankCapAsksEveryRowAndADigitCapAsksThatMany() {
        #expect(SavedSearchCap.typed("") == SavedSearchCap(rows: nil))
        #expect(SavedSearchCap.typed("   ") == SavedSearchCap(rows: nil))
        #expect(SavedSearchCap.typed(" 25 ") == SavedSearchCap(rows: 25))
    }

    @Test(arguments: ["0", "-3", "2.5", "many", "99999999999"])
    func aCapThatIsNotAWholeNumberAboveZeroIsRefused(typed: String) {
        #expect(SavedSearchCap.typed(typed) == nil)
    }

    @Test func aDraftReadsBackAsTheSearchItWasMadeFrom() {
        let search = SavedSearch(text: "genre:jazz", order: .plays, descending: true, rows: 40)

        let draft = SavedSearchDraft.of(name: "Jazz", search: search)

        #expect(draft.name == "Jazz")
        #expect(draft.cap == "40")
        #expect(draft.search == search)
    }

    @Test func aDraftTrimsItsTextAndIsNotSavableWithABadCapOrNoName() {
        var draft = SavedSearchDraft(name: " Jazz ", text: "  genre:jazz ", order: .title, descending: false, cap: "")

        #expect(draft.search == SavedSearch(text: "genre:jazz", order: .title, descending: false, rows: nil))
        #expect(draft.trimmedName == "Jazz")
        #expect(draft.savable)

        draft.cap = "0"
        #expect(draft.search == nil)
        #expect(!draft.savable)

        draft.cap = ""
        draft.name = "  "
        #expect(!draft.savable)
    }

    @Test func choosingAnOrderTakesItsNaturalDirection() {
        var draft = SavedSearchDraft.starting(name: "Jazz", text: "genre:jazz")

        draft.choose(.plays)

        #expect(draft.order == .plays)
        #expect(draft.descending == searchOrderReadsDescending(order: .plays))
    }

    @Test func everyOrderIsOfferedOnceUnderItsOwnTitle() {
        #expect(Set(SearchOrder.offered).count == 10)
        #expect(Set(SearchOrder.offered.map(\.label)).count == 10)
    }
}
