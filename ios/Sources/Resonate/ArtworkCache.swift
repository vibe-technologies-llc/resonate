import Foundation
import ImageIO
import UIKit

enum ArtworkBucket: Int, CaseIterable, Sendable {
    case row = 96
    case tile = 192
    case card = 384
    case shelf = 512
    case page = 768
    case full = 1024

    static func fitting(points: CGFloat, scale: CGFloat) -> ArtworkBucket {
        let pixels = (points * scale).rounded(.up)
        return allCases.first { CGFloat($0.rawValue) >= pixels } ?? .full
    }
}

enum ArtworkSubject: Hashable, Sendable {
    case track(UInt64)
    case portrait(String)
    case picture(UInt64)
}

struct ArtworkRequest: Hashable, Sendable {
    let subject: ArtworkSubject
    let revision: UInt64
    let generation: Int
    let bucket: ArtworkBucket

    var name: NSString {
        switch subject {
        case .track(let id): "track:\(id):\(revision):\(generation):\(bucket.rawValue)" as NSString
        case .portrait(let mbid): "portrait:\(mbid):\(generation):\(bucket.rawValue)" as NSString
        case .picture(let id): "picture:\(id):\(bucket.rawValue)" as NSString
        }
    }

    var songName: NSString {
        switch subject {
        case .track(let id): "song:\(revision):\(generation):track:\(id)" as NSString
        case .portrait(let mbid): "song:\(revision):\(generation):portrait:\(mbid)" as NSString
        case .picture(let id): "song:\(revision):\(generation):picture:\(id)" as NSString
        }
    }

    func of(picture: UInt64) -> ArtworkRequest {
        ArtworkRequest(subject: .picture(picture), revision: 0, generation: 0, bucket: bucket)
    }
}

final class PicturesOfSongs: @unchecked Sendable {
    private static let songsAtMost = 20_000

    private final class Held {
        let picture: UInt64?

        init(_ picture: UInt64?) {
            self.picture = picture
        }
    }

    private let cache: NSCache<NSString, Held> = {
        let cache = NSCache<NSString, Held>()
        cache.countLimit = songsAtMost
        return cache
    }()

    func picture(of request: ArtworkRequest) -> UInt64?? {
        cache.object(forKey: request.songName).map(\.picture)
    }

    func keep(_ picture: UInt64?, for request: ArtworkRequest) {
        cache.setObject(Held(picture), forKey: request.songName)
    }
}

actor DecodeSlots {
    private struct Waiter {
        let id: UInt64
        let continuation: CheckedContinuation<Void, any Swift.Error>
    }

    private var free: Int
    private var waiters: [Waiter] = []
    private var issued: UInt64 = 0

    init(_ count: Int) {
        free = count
    }

    func acquire() async throws {
        try Task.checkCancellation()
        if free > 0 {
            free -= 1
            return
        }
        issued += 1
        let id = issued
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Swift.Error>) in
                if Task.isCancelled {
                    continuation.resume(throwing: CancellationError())
                } else {
                    waiters.append(Waiter(id: id, continuation: continuation))
                }
            }
        } onCancel: {
            Task { await self.abandon(id) }
        }
    }

    func release() {
        if waiters.isEmpty {
            free += 1
        } else {
            waiters.removeFirst().continuation.resume()
        }
    }

    private func abandon(_ id: UInt64) {
        guard let index = waiters.firstIndex(where: { $0.id == id }) else { return }
        waiters.remove(at: index).continuation.resume(throwing: CancellationError())
    }
}

final class DecodedPictures: @unchecked Sendable {
    private static let costLimit = 160 * 1024 * 1024

    private let cache: NSCache<NSString, UIImage> = {
        let cache = NSCache<NSString, UIImage>()
        cache.totalCostLimit = costLimit
        return cache
    }()

    func object(forKey name: NSString) -> UIImage? {
        cache.object(forKey: name)
    }

    func setObject(_ image: UIImage, forKey name: NSString, cost: Int) {
        cache.setObject(image, forKey: name, cost: cost)
    }

    func removeObject(forKey name: NSString) {
        cache.removeObject(forKey: name)
    }
}

actor ArtworkCache {
    private static let decodesAtOnce = 4
    private static let lookupsAtOnce = 4

    private struct Flight {
        let task: Task<UIImage?, Never>
        var waiting: Int
    }

    private let pictures = DecodedPictures()
    private let missing = NSCache<NSString, NSNull>()
    private let songs = PicturesOfSongs()
    private let slots = DecodeSlots(decodesAtOnce)
    private let lookups = DecodeSlots(lookupsAtOnce)
    private var flights: [ArtworkRequest: Flight] = [:]

    nonisolated func cached(_ request: ArtworkRequest) -> UIImage? {
        guard case .track = request.subject else { return pictures.object(forKey: request.name) }
        guard let picture = songs.picture(of: request) ?? nil else { return nil }
        return pictures.object(forKey: request.of(picture: picture).name)
    }

    func image(
        _ request: ArtworkRequest,
        identifying: @escaping @Sendable () throws -> UInt64?,
        reading: @escaping @Sendable (UInt64) throws -> Data?
    ) async -> UIImage? {
        guard let picture = await picture(of: request, identifying: identifying) else { return nil }
        return await image(request.of(picture: picture)) { try reading(picture) }
    }

    func unkept(
        _ request: ArtworkRequest,
        identifying: @escaping @Sendable () throws -> UInt64?,
        reading: @escaping @Sendable (UInt64) throws -> Data?
    ) async -> UIImage? {
        if let held = cached(request) { return held }
        guard let picture = await picture(of: request, identifying: identifying) else { return nil }
        do {
            try await slots.acquire()
        } catch {
            return nil
        }
        let edge = request.bucket.rawValue
        let decoded = try? await Work.onScreen { ArtworkDecoder.decoded(try reading(picture), edge: edge) }
        await slots.release()
        return decoded?.image
    }

    private func picture(
        of request: ArtworkRequest,
        identifying: @escaping @Sendable () throws -> UInt64?
    ) async -> UInt64? {
        if let known = songs.picture(of: request) { return known }
        do {
            try await lookups.acquire()
        } catch {
            return nil
        }
        let found: UInt64??
        do {
            found = try await Work.onScreen(identifying)
        } catch {
            found = nil
        }
        await lookups.release()

        guard let found else { return nil }
        songs.keep(found, for: request)
        return found
    }

    func image(
        _ request: ArtworkRequest,
        reading: @escaping @Sendable () throws -> Data?
    ) async -> UIImage? {
        let name = request.name
        if let picture = pictures.object(forKey: name) { return picture }
        if missing.object(forKey: name) != nil { return nil }

        let flight = flights[request] ?? Flight(task: decoding(request, reading: reading), waiting: 0)
        flights[request] = Flight(task: flight.task, waiting: flight.waiting + 1)

        let image = await withTaskCancellationHandler {
            await flight.task.value
        } onCancel: {
            Task { await self.leave(request, flight.task) }
        }
        settle(request, flight.task)
        return Task.isCancelled ? nil : image
    }

    private func decoding(
        _ request: ArtworkRequest,
        reading: @escaping @Sendable () throws -> Data?
    ) -> Task<UIImage?, Never> {
        let slots = slots
        let edge = request.bucket.rawValue
        return Task.detached(priority: .userInitiated) { [weak self] in
            do {
                try await slots.acquire()
            } catch {
                return nil
            }
            let decoded: DecodedArtwork
            do {
                decoded = try await Work.onScreen { ArtworkDecoder.decoded(try reading(), edge: edge) }
            } catch {
                decoded = .unread
            }
            await slots.release()
            await self?.keep(decoded, for: request)
            return decoded.image
        }
    }

    private func keep(_ decoded: DecodedArtwork, for request: ArtworkRequest) {
        let name = request.name
        switch decoded {
        case .image(let image):
            pictures.setObject(image, forKey: name, cost: ArtworkDecoder.cost(of: image))
        case .absent:
            missing.setObject(NSNull(), forKey: name)
        case .unread:
            return
        }
    }

    private func leave(_ request: ArtworkRequest, _ task: Task<UIImage?, Never>) {
        guard var flight = flights[request], flight.task == task else { return }
        flight.waiting -= 1
        if flight.waiting > 0 {
            flights[request] = flight
        } else {
            flights[request] = nil
            task.cancel()
        }
    }

    private func settle(_ request: ArtworkRequest, _ task: Task<UIImage?, Never>) {
        if flights[request]?.task == task { flights[request] = nil }
    }
}

enum DecodedArtwork: Sendable {
    case image(UIImage)
    case absent
    case unread

    var image: UIImage? {
        guard case .image(let image) = self else { return nil }
        return image
    }
}

enum ArtworkDecoder {
    static func decoded(_ bytes: Data?, edge: Int) -> DecodedArtwork {
        bytes.flatMap { thumbnail($0, edge: edge) }.map(DecodedArtwork.image) ?? .absent
    }

    static func thumbnail(_ bytes: Data, edge: Int) -> UIImage? {
        guard let source = CGImageSourceCreateWithData(bytes as CFData, nil) else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceThumbnailMaxPixelSize: edge,
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
        let picture = UIImage(cgImage: image)
        return picture.preparingForDisplay() ?? picture
    }

    static func cost(of image: UIImage) -> Int {
        guard let bitmap = image.cgImage else { return 1 }
        return max(bitmap.bytesPerRow * bitmap.height, 1)
    }
}
