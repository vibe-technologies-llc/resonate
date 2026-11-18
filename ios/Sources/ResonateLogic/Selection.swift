public struct RowSelection<Key: Hashable & Sendable>: Equatable, Sendable {
    public private(set) var selecting = false
    public private(set) var chosen: Set<Key> = []

    public init() {}

    public var count: Int { chosen.count }

    public func contains(_ key: Key) -> Bool {
        chosen.contains(key)
    }

    public func picked<Row>(_ rows: [Row], key: (Row) -> Key) -> [Row] {
        rows.filter { chosen.contains(key($0)) }
    }

    public mutating func begin() {
        selecting = true
    }

    public mutating func begin(with key: Key) {
        selecting = true
        chosen.insert(key)
    }

    public mutating func toggle(_ key: Key) {
        guard selecting else { return }
        if chosen.remove(key) == nil { chosen.insert(key) }
    }

    public mutating func selectAll(_ keys: some Sequence<Key>) {
        selecting = true
        chosen = Set(keys)
    }

    public mutating func clear() {
        selecting = false
        chosen = []
    }

    public mutating func retain(_ kept: Set<Key>) {
        chosen.formIntersection(kept)
    }
}
