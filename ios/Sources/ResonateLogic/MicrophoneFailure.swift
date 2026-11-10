import Foundation

public enum MicrophoneFailure: LocalizedError {
    case unavailable
    case interrupted

    public var errorDescription: String? {
        switch self {
        case .unavailable: said("microphone.unavailable")
        case .interrupted: said("microphone.interrupted")
        }
    }
}
