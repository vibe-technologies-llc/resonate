import Foundation
import ResonateNative

extension Track {
    public var shownTitle: String { title.isEmpty ? name : title }
    public var shownArtist: String { artist.isEmpty ? said("library.unknown.artist") : artist }
    public var groupingArtist: String { albumArtist.isEmpty ? artist : albumArtist }

    public var quality: String {
        var parts = [codec]
        if let bits { parts.append(said("units.bits", Int(bits))) }
        if rate > 0 { parts.append(said("units.khz", String(format: "%g", Double(rate) / 1000))) }
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

extension AlbumHit {
    public var shownTitle: String { title.isEmpty ? said("library.unknown.album") : title }
    public var shownArtist: String { artist.isEmpty ? said("library.unknown.artist") : artist }

    public var detail: String {
        [shownArtist, year.map(String.init) ?? "", said("counted.songs", Int(songs))]
            .filter { !$0.isEmpty }
            .joined(separator: " · ")
    }
}

extension ArtistHit {
    public var shownName: String { name.isEmpty ? said("library.unknown.artist") : name }

    public var detail: String {
        albums > 0
            ? "\(said("counted.albums", Int(albums))) · \(said("counted.songs", Int(songs)))"
            : said("counted.songs", Int(songs))
    }
}
