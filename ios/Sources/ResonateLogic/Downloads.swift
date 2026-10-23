import Foundation
import ResonateNative

public enum DownloadState: Equatable {
    case waiting
    case finding
    case downloading
    case validating
    case done(Track)
    case failed(String)
    case cancelled

    public var inFlight: Bool {
        switch self {
        case .finding, .downloading, .validating: true
        default: false
        }
    }

    public var settled: Bool {
        switch self {
        case .done, .failed, .cancelled: true
        default: false
        }
    }
}

public struct DownloadJob: Identifiable, Equatable {
    public let id: UInt64
    public let song: SearchSong
    public let folder: MusicFolder
    public var state = DownloadState.waiting
    public var received: UInt64 = 0
    public var total: UInt64?
    public var provider: MusicProvider?

    public init(id: UInt64, song: SearchSong, folder: MusicFolder) {
        self.id = id
        self.song = song
        self.folder = folder
    }

    public var fraction: Double? {
        guard let total, total > 0 else { return nil }
        return min(Double(received) / Double(total), 1)
    }

    public mutating func follow(_ progress: ResonateNative.DownloadProgress) {
        received = progress.received
        total = progress.total ?? total
        provider = progress.provider ?? provider
        guard state.inFlight else { return }
        switch progress.phase {
        case .finding: state = .finding
        case .downloading: state = .downloading
        case .validating: state = .validating
        case .done, .failed, .cancelled: return
        }
    }
}

public enum ProviderReadiness: Equatable {
    case ready
    case missingServer
    case missingLogin
    case missingClient
    case needsSignIn
}

extension DownloadAccount {
    public var readiness: ProviderReadiness {
        switch provider {
        case .monochrome, .hifiApi: .ready
        case .subsonic where Self.isBlank(server): .missingServer
        case .subsonic where Self.isBlank(user) || password.isEmpty: .missingLogin
        case .subsonic: .ready
        case .tidal where Self.isBlank(clientId): .missingClient
        case .tidal where Self.isBlank(refreshToken): .needsSignIn
        case .tidal: .ready
        }
    }

    public var usesItsOwnServer: Bool {
        !Self.isBlank(server) || (provider == .hifiApi && !Self.isBlank(password))
    }

    private static func isBlank(_ value: String) -> Bool {
        value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
}

public struct DownloadJournal: Sendable {
    public static let key = "unfinished-download"
    public static let suite = "library-features"

    private let suiteName: String

    public init(suite: String = DownloadJournal.suite) {
        suiteName = suite
    }

    private var settings: UserDefaults { UserDefaults(suiteName: suiteName) ?? .standard }

    public var uris: [String] {
        settings.stringArray(forKey: Self.key) ?? settings.string(forKey: Self.key).map { [$0] } ?? []
    }

    public func change(_ change: (inout [String]) -> Void) {
        var uris = settings.stringArray(forKey: Self.key) ?? []
        change(&uris)
        keep(uris)
    }

    public func settleRecovery(left: [String], kept: [String]) {
        keep(uris.filter { !left.contains($0) || kept.contains($0) })
    }

    private func keep(_ uris: [String]) {
        if uris.isEmpty {
            settings.removeObject(forKey: Self.key)
        } else {
            settings.set(uris, forKey: Self.key)
        }
    }
}
