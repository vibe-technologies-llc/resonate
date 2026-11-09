import Foundation
import ResonateNative

public enum Suggestions {
    public static let homeAtMost = 12

    public static let homeShelves: Set<SuggestionShelf> = [.forYou, .rediscover, .listening]

    public static var yoursShelf: String { said("suggestions.shelf.yours") }

    public static func home(_ lists: [SuggestedList]) -> [SuggestedList] {
        Array(lists.filter { homeShelves.contains($0.shelf) }.prefix(homeAtMost))
    }

    public static func symbol(for reason: SuggestionReason) -> String {
        switch reason {
        case .decade, .genreOfADecade: "calendar"
        case .genre: "guitars"
        case .artist: "music.mic"
        case .neverHeard: "sparkles"
        case .mostPlayed: "chart.bar.fill"
        case .recentlyAdded, .freshArrivals: "clock.fill"
        case .backCatalogue, .oldFlames, .artistLeftBehind: "clock.arrow.circlepath"
        case .onRepeat: "repeat"
        case .kindredArtists: "person.2.fill"
        case .deepCuts: "opticaldisc"
        case .artistToDiscover: "binoculars.fill"
        case .unheardGenre: "music.note.list"
        case .forgottenFavourites: "heart.fill"
        case .secondChances: "arrow.counterclockwise"
        case .starredUnheard: "star.fill"
        case .hiRes: "waveform"
        case .longPlayers, .slowBurners, .shortAndSharp: "timer"
        case .surround: "hifispeaker.2.fill"
        case .mono: "speaker.fill"
        }
    }
}
