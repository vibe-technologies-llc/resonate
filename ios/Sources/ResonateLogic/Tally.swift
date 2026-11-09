import Foundation

public final class Tally: @unchecked Sendable {
    public enum Companion {
        case cover
        case lyrics
    }

    private static let postedEvery = Duration.milliseconds(200)

    private let lock = NSLock()
    private var read = 0
    private var posted: ContinuousClock.Instant?
    private var unreadable = 0
    private var firstUnreadable: String?
    private var companions: [Companion: Int] = [:]
    private var firstCompanion: String?
    private var unreached = 0
    private var firstUnreached: String?

    public init() {}

    public var complete: Bool {
        lock.withLock { unreached == 0 }
    }

    public func skipped(_ path: String, _ reason: any Swift.Error) {
        lock.withLock {
            unreached += 1
            if firstUnreached == nil { firstUnreached = "\(path): \(explained(reason))" }
        }
    }

    public func indexed() -> Int? {
        lock.withLock {
            read += 1
            let now = ContinuousClock.now
            if let posted, posted.duration(to: now) < Self.postedEvery { return nil }
            posted = now
            return read
        }
    }

    public func failed(_ name: String, _ reason: any Swift.Error) {
        lock.withLock {
            unreadable += 1
            if firstUnreadable == nil { firstUnreadable = "\(name): \(explained(reason))" }
        }
    }

    public func lost(_ kind: Companion, _ name: String, _ reason: any Swift.Error) {
        lock.withLock {
            companions[kind, default: 0] += 1
            if firstCompanion == nil { firstCompanion = "\(name): \(explained(reason))" }
        }
    }

    public var count: Int {
        lock.withLock { read }
    }

    public var summary: String {
        lock.withLock { said("tally.read", read) + trouble }
    }

    public var foundSummary: String {
        lock.withLock { said("tally.found", read) + trouble }
    }

    public var stoppedSummary: String {
        lock.withLock { said("tally.stopped", read) + trouble }
    }

    private var trouble: String {
        var told = ""
        if unreached > 0 {
            told += "; " + said("tally.unreached", unreached) + (firstUnreached.map { " (\($0))" } ?? "")
        }
        if unreadable > 0 {
            told += "; " + said("tally.unreadable", unreadable) + (firstUnreadable.map { " (\($0))" } ?? "")
        }
        let covers = companions[.cover] ?? 0
        let sheets = companions[.lyrics] ?? 0
        if covers + sheets > 0 {
            let coversLost = said("counted.covers", covers)
            let sheetsLost = said("counted.lyrics-sheets", sheets)
            let lost = covers == 0 ? sheetsLost : sheets == 0 ? coversLost : said("tally.lost.both", coversLost, sheetsLost)
            told += "; " + said("tally.lost", lost) + (firstCompanion.map { " (\($0))" } ?? "")
        }
        return told
    }
}
