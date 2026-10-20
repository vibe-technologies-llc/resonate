import Foundation

public enum MicrophoneFailure: LocalizedError {
    case unavailable
    case interrupted

    public var errorDescription: String? {
        switch self {
        case .unavailable: "No microphone is available to listen with."
        case .interrupted: "Listening was interrupted by a call, a change of headphones or another app."
        }
    }
}
