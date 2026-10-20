import Foundation
import ResonateNative

public enum ListenStage: Equatable {
    case idle
    case recording(nth: Int, since: Date)
    case asking
    case found(HeardSong)
    case unknown
    case silent
    case unreached(String)
    case offline
    case denied
    case failed(String)

    public var isListening: Bool {
        switch self {
        case .recording, .asking: true
        default: false
        }
    }

    public var namedOrMissed: Bool {
        switch self {
        case .found, .unknown: true
        default: false
        }
    }
}

public enum ListenFollowing: Equatable {
    case off
    case waiting
    case listening
}

public enum ListenNext: Equatable {
    case tell(HeardSong)
    case again(nth: Int)
    case settle(ListenStage)
    case keep

    public static let triesOnAMiss = 3

    public static func after(_ answer: HeardAnswer, naming nth: Int?, standing: HeardSong?) -> ListenNext {
        switch (nth, answer) {
        case (nil, .named(let song)) where standing?.key == song.key: .keep
        case (_, .named(let song)): .tell(song)
        case (nil, _): .keep
        case (let nth?, .missed) where nth < triesOnAMiss, (let nth?, .silent) where nth < triesOnAMiss:
            .again(nth: nth + 1)
        case (_?, .missed): .settle(.unknown)
        case (_?, .silent): .settle(.silent)
        case (_?, .unreached(let service)): .settle(.unreached(service))
        }
    }
}
