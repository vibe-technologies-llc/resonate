public enum Paging {
    public static let walksAtMost = 3

    public static func pinned<Row, Key: Hashable>(
        pageRows: Int,
        key: (Row) -> Key,
        page: (_ offset: UInt32, _ pinned: UInt64?) throws -> (rows: [Row], revision: UInt64)
    ) rethrows -> [Row] {
        var rows: [Row] = []
        for walk in 1...walksAtMost {
            rows = []
            var pinned: UInt64?
            var lost = false
            while true {
                let read = try page(UInt32(rows.count), pinned)
                if let held = pinned, read.revision != held {
                    lost = true
                    if walk < walksAtMost { break }
                }
                pinned = read.revision
                rows += read.rows
                if read.rows.count < pageRows { break }
            }
            if !lost { break }
        }
        var seen = Set<Key>()
        return rows.filter { seen.insert(key($0)).inserted }
    }
}
