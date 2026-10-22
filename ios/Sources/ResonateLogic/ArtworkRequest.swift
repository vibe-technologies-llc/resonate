import Foundation

#if canImport(CoreGraphics)
import CoreGraphics
#endif

public enum ArtworkBucket: Int, CaseIterable, Sendable {
    case row = 96
    case tile = 192
    case card = 384
    case shelf = 512
    case page = 768
    case full = 1024

    public static func fitting(points: CGFloat, scale: CGFloat) -> ArtworkBucket {
        let pixels = (points * scale).rounded(.up)
        return allCases.first { CGFloat($0.rawValue) >= pixels } ?? .full
    }
}

public enum ArtworkSubject: Hashable, Sendable {
    case track(UInt64)
    case portrait(String)
    case picture(UInt64)
}

public struct ArtworkRequest: Hashable, Sendable {
    public let subject: ArtworkSubject
    public let revision: UInt64
    public let generation: Int
    public let bucket: ArtworkBucket

    public init(subject: ArtworkSubject, revision: UInt64, generation: Int, bucket: ArtworkBucket) {
        self.subject = subject
        self.revision = revision
        self.generation = generation
        self.bucket = bucket
    }

    public var name: NSString {
        switch subject {
        case .track(let id): "track:\(id):\(revision):\(generation):\(bucket.rawValue)" as NSString
        case .portrait(let mbid): "portrait:\(mbid):\(generation):\(bucket.rawValue)" as NSString
        case .picture(let id): "picture:\(id):\(bucket.rawValue)" as NSString
        }
    }

    public var songName: NSString {
        switch subject {
        case .track(let id): "song:\(revision):\(generation):track:\(id)" as NSString
        case .portrait(let mbid): "song:\(revision):\(generation):portrait:\(mbid)" as NSString
        case .picture(let id): "song:\(revision):\(generation):picture:\(id)" as NSString
        }
    }

    public func of(picture: UInt64) -> ArtworkRequest {
        ArtworkRequest(subject: .picture(picture), revision: 0, generation: 0, bucket: bucket)
    }
}
