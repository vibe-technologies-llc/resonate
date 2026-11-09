public enum Paging {
    public static let walksAtMost = 3

    public static func steadily<Row, Key: Hashable>(
        revision: () -> UInt64,
        key: (Row) -> Key,
        everyPage: () throws -> [Row]
    ) rethrows -> [Row] {
        var rows: [Row] = []
        for _ in 0..<walksAtMost {
            let before = revision()
            rows = try everyPage()
            if revision() == before { break }
        }
        var seen = Set<Key>()
        return rows.filter { seen.insert(key($0)).inserted }
    }
}
