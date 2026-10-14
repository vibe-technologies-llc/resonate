import Foundation
import ImageIO
import UIKit

enum ArtworkBucket: Int, CaseIterable, Sendable {
    case row = 96
    case tile = 192
    case card = 384
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
        }
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

actor ArtworkCache {
    private static let costLimit = 32 * 1024 * 1024
    private static let decodesAtOnce = 4

    private struct Flight {
        let task: Task<UIImage?, Never>
        var waiting: Int
    }

    private let pictures: NSCache<NSString, UIImage> = {
        let cache = NSCache<NSString, UIImage>()
        cache.totalCostLimit = costLimit
        return cache
    }()
    private let missing = NSCache<NSString, NSNull>()
    private let slots = DecodeSlots(decodesAtOnce)
    private var kept: [ArtworkSubject: Set<NSString>] = [:]
    private var flights: [ArtworkRequest: Flight] = [:]

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

    func forget(_ subjects: [ArtworkSubject]) {
        for subject in subjects {
            for name in kept.removeValue(forKey: subject) ?? [] {
                pictures.removeObject(forKey: name)
                missing.removeObject(forKey: name)
            }
        }
    }

    func forgetPortraits() {
        forget(kept.keys.filter { if case .portrait = $0 { true } else { false } })
    }

    private func decoding(
        _ request: ArtworkRequest,
        reading: @escaping @Sendable () throws -> Data?
    ) -> Task<UIImage?, Never> {
        let slots = slots
        let edge = request.bucket.rawValue
        return Task.detached(priority: .utility) { [weak self] in
            do {
                try await slots.acquire()
            } catch {
                return nil
            }
            let decoded: DecodedArtwork
            do {
                decoded = try await Work.aside { ArtworkDecoder.decoded(try reading(), edge: edge) }
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
        kept[request.subject, default: []].insert(name)
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
        return UIImage(cgImage: image)
    }

    static func cost(of image: UIImage) -> Int {
        guard let bitmap = image.cgImage else { return 1 }
        return max(bitmap.bytesPerRow * bitmap.height, 1)
    }
}
