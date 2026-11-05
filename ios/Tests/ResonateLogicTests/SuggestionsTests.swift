import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct SuggestionsTests {
    private static func list(_ name: String, _ reason: SuggestionReason, _ shelf: SuggestionShelf) -> SuggestedList {
        SuggestedList(
            id: name,
            name: name,
            detail: "",
            reason: reason,
            shelf: shelf,
            shelfName: "",
            search: SavedSearch(text: "", order: .relevance, descending: false, rows: nil),
            songs: 10,
            durationMs: 0,
            covers: [],
            accents: MixAccents(from: .teal, to: .blue)
        )
    }

    @Test func homeShowsTheListsFromListeningAndLeavesTheCatalogsShelvesToTheSuggestionsPage() {
        let lists = [
            Self.list("On repeat", .onRepeat, .forYou),
            Self.list("The eighties", .decade, .eras),
            Self.list("Old flames", .oldFlames, .rediscover),
            Self.list("Rock", .genre, .genres),
            Self.list("Never heard", .neverHeard, .listening),
        ]

        #expect(Suggestions.home(lists).map(\.name) == ["On repeat", "Old flames", "Never heard"])
    }

    @Test func homeShowsNoMoreThanItsShelfHolds() {
        let lists = (0..<30).map { Self.list("List \($0)", .onRepeat, .forYou) }

        #expect(Suggestions.home(lists).count == Suggestions.homeAtMost)
        #expect(Suggestions.home(lists).first?.name == "List 0")
    }

    @Test func everyReasonHasASymbol() {
        let reasons: [SuggestionReason] = [
            .decade, .genreOfADecade, .genre, .artist, .neverHeard, .mostPlayed, .recentlyAdded,
            .freshArrivals, .backCatalogue, .onRepeat, .kindredArtists, .deepCuts, .artistToDiscover,
            .unheardGenre, .forgottenFavourites, .oldFlames, .secondChances, .starredUnheard,
            .artistLeftBehind, .hiRes, .longPlayers, .slowBurners, .shortAndSharp, .surround, .mono,
        ]

        for reason in reasons {
            #expect(!Suggestions.symbol(for: reason).isEmpty)
        }
        #expect(Suggestions.symbol(for: .onRepeat) == "repeat")
    }
}
