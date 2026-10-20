import Foundation

public enum SettingsSearch {
    public static func terms(_ search: String) -> [String] {
        search.split(whereSeparator: \.isWhitespace).map(String.init)
    }

    public static func matches(_ searched: String, terms: [String]) -> Bool {
        terms.allSatisfy { searched.localizedCaseInsensitiveContains($0) }
    }
}

public enum RowMoves {
    public static func queue(offset moved: Int, destination: Int, within lower: Int) -> (from: Int, to: Int) {
        (lower + moved, lower + destination)
    }

    public static func playlist(from start: Int, destination to: Int) -> Int {
        to > start ? to - 1 : to
    }
}
