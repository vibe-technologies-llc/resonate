import ResonateNative

public struct SavedSearchCap: Equatable, Sendable {
    public let rows: UInt32?

    public static func typed(_ text: String) -> SavedSearchCap? {
        let digits = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if digits.isEmpty { return SavedSearchCap(rows: nil) }
        guard digits.allSatisfy({ $0 >= "0" && $0 <= "9" }), let rows = UInt32(digits), rows > 0 else { return nil }
        return SavedSearchCap(rows: rows)
    }
}

public struct SavedSearchDraft: Equatable, Sendable {
    public var name: String
    public var text: String
    public var order: SearchOrder
    public var descending: Bool
    public var cap: String

    public init(name: String, text: String, order: SearchOrder, descending: Bool, cap: String) {
        self.name = name
        self.text = text
        self.order = order
        self.descending = descending
        self.cap = cap
    }

    public static func starting(name: String, text: String) -> SavedSearchDraft {
        SavedSearchDraft(
            name: name,
            text: text,
            order: .relevance,
            descending: searchOrderReadsDescending(order: .relevance),
            cap: ""
        )
    }

    public static func of(name: String, search: SavedSearch) -> SavedSearchDraft {
        SavedSearchDraft(
            name: name,
            text: search.text,
            order: search.order,
            descending: search.descending,
            cap: search.rows.map { String($0) } ?? ""
        )
    }

    public var trimmedName: String {
        name.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    public var search: SavedSearch? {
        SavedSearchCap.typed(cap).map {
            SavedSearch(
                text: text.trimmingCharacters(in: .whitespacesAndNewlines),
                order: order,
                descending: descending,
                rows: $0.rows
            )
        }
    }

    public var savable: Bool {
        search != nil && !trimmedName.isEmpty
    }

    public mutating func choose(_ chosen: SearchOrder) {
        order = chosen
        descending = searchOrderReadsDescending(order: chosen)
    }
}

extension SearchOrder {
    public static let offered: [SearchOrder] = [
        .relevance, .album, .title, .artist, .added, .duration, .plays, .played, .favourites, .playsThisMonth,
    ]

    public var label: String {
        switch self {
        case .relevance: "Relevance"
        case .album: "Album"
        case .title: "Title"
        case .artist: "Artist"
        case .added: "Date added"
        case .duration: "Duration"
        case .plays: "Plays"
        case .played: "Last played"
        case .favourites: "Favourites"
        case .playsThisMonth: "Plays this month"
        }
    }
}

extension SavedSearch {
    public var reading: String {
        let natural = searchOrderReadsDescending(order: order)
        let direction = descending == natural ? "" : descending ? ", descending" : ", ascending"
        let limit = rows.map { " · first \($0)" } ?? ""
        return order.label + direction + limit
    }
}
