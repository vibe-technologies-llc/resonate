import ResonateNative

public enum TagName: CaseIterable, Hashable, Sendable {
    case title, artist, album, albumArtist, genre, date, trackNumber, trackTotal, discNumber, discTotal, composer

    public var label: String {
        switch self {
        case .title: "Title"
        case .artist: "Artist"
        case .album: "Album"
        case .albumArtist: "Album artist"
        case .genre: "Genre"
        case .date: "Year or date"
        case .trackNumber: "Track"
        case .trackTotal: "of"
        case .discNumber: "Disc"
        case .discTotal: "of"
        case .composer: "Composer"
        }
    }

    public var numbered: Bool {
        switch self {
        case .trackNumber, .trackTotal, .discNumber, .discTotal: true
        default: false
        }
    }

    public var eachSongsOwn: Bool {
        self == .title || self == .trackNumber
    }

    public var path: WritableKeyPath<SongTags, String?> {
        switch self {
        case .title: \.title
        case .artist: \.artist
        case .album: \.album
        case .albumArtist: \.albumArtist
        case .genre: \.genre
        case .date: \.date
        case .trackNumber: \.trackNumber
        case .trackTotal: \.trackTotal
        case .discNumber: \.discNumber
        case .discTotal: \.discTotal
        case .composer: \.composer
        }
    }
}

public enum TagDiff {
    public static func common(of all: [SongTags]) -> (common: [TagName: String], differing: Set<TagName>) {
        var common: [TagName: String] = [:]
        var differing: Set<TagName> = []
        for name in TagName.allCases {
            let values = Set(all.map { $0[keyPath: name.path] ?? "" })
            if values.count == 1, let only = values.first {
                common[name] = only
            } else if values.count > 1 {
                differing.insert(name)
            }
        }
        return (common, differing)
    }

    public static func changed(
        shown: [TagName: String],
        read: [TagName: String],
        mixed: Set<TagName>,
        emptied: Set<TagName> = []
    ) -> [TagName] {
        TagName.allCases.filter { name in
            let now = shown[name] ?? ""
            return mixed.contains(name) ? !now.isEmpty || emptied.contains(name) : now != (read[name] ?? "")
        }
    }

    public static func change(shown: [TagName: String], changed: [TagName]) -> SongTags {
        changed.reduce(into: SongTags()) { tags, name in
            tags[keyPath: name.path] = shown[name] ?? ""
        }
    }
}
