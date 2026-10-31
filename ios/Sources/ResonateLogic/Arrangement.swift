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

    public static func joined(offset moved: Int, destination: Int, joint: Int, within lower: Int) -> (from: Int, to: Int)? {
        guard moved != joint else { return nil }
        let from = moved < joint ? moved : moved - 1
        let to = destination <= joint ? destination : destination - 1
        return (lower + from, lower + to)
    }

    public static func playlist(from start: Int, destination to: Int) -> Int {
        to > start ? to - 1 : to
    }
}
