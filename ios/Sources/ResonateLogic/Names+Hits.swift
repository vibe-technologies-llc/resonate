import Foundation
import ResonateNative

extension Track {
    public var shownTitle: String { title.isEmpty ? name : title }
    public var shownArtist: String { artist.isEmpty ? "Unknown artist" : artist }
    public var groupingArtist: String { albumArtist.isEmpty ? artist : albumArtist }

    public var quality: String {
        var parts = [codec]
        if let bits { parts.append("\(bits)-bit") }
        if rate > 0 { parts.append(String(format: "%g kHz", Double(rate) / 1000)) }
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

extension AlbumHit {
    public var shownTitle: String { title.isEmpty ? "Unknown album" : title }
    public var shownArtist: String { artist.isEmpty ? "Unknown artist" : artist }

    public var detail: String {
        [shownArtist, year.map(String.init) ?? "", counted(Int(songs), "song")]
            .filter { !$0.isEmpty }
            .joined(separator: " · ")
    }
}

extension ArtistHit {
    public var shownName: String { name.isEmpty ? "Unknown artist" : name }

    public var detail: String {
        albums > 0 ? "\(counted(Int(albums), "album")) · \(counted(Int(songs), "song"))" : counted(Int(songs), "song")
    }
}
