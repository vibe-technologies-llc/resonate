import Foundation
import ResonateNative

public func explained(_ failure: any Swift.Error) -> String {
    if let native = failure as? ResonateNative.Error,
        let reason = Mirror(reflecting: native).children.first
    {
        if let message = reason.value as? String {
            return message
        }
        if let message = Mirror(reflecting: reason.value).children.first?.value as? String {
            return message
        }
    }
    return failure.localizedDescription
}

public func counted(_ count: Int, _ noun: String) -> String {
    "\(count) \(count == 1 ? noun : noun + "s")"
}

public func clock(_ milliseconds: UInt64) -> String {
    let seconds = milliseconds / 1000
    let hours = seconds / 3600
    let minutes = seconds / 60 % 60
    let rest = seconds % 60
    return hours > 0
        ? String(format: "%d:%02d:%02d", hours, minutes, rest)
        : String(format: "%d:%02d", minutes, rest)
}
