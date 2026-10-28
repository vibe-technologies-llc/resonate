import Foundation
import ResonateNative

public enum MeasurementAim: Equatable, Sendable {
    case builtIn(name: String)
    case document(name: String, bytes: Data)

    public var target: MeasurementTarget {
        switch self {
        case .builtIn(let name): .builtIn(name: name)
        case .document(_, let bytes): .document(bytes: bytes)
        }
    }

    public func label(among targets: [BuiltInTarget]) -> String {
        switch self {
        case .builtIn(let name): targets.first { $0.name == name }?.label ?? name
        case .document(let name, _): name
        }
    }
}

public struct MeasurementView: Equatable, Sendable {
    public let offers: MeasurementOffers
    public var aim: MeasurementAim?
    public var fitting: Bool
    public var said: String?

    public static func of(_ offers: MeasurementOffers) -> MeasurementView {
        let first = offers.targets.first { $0.recommended } ?? offers.targets.first
        return MeasurementView(
            offers: offers,
            aim: first.map { .builtIn(name: $0.name) },
            fitting: false,
            said: nil
        )
    }

    public func aimed(at aim: MeasurementAim) -> MeasurementView {
        var next = self
        next.aim = aim
        return next
    }

    public func begun() -> MeasurementView {
        var next = self
        next.fitting = true
        next.said = nil
        return next
    }

    public func finished(saying said: String?) -> MeasurementView {
        var next = self
        next.fitting = false
        next.said = said ?? self.said
        return next
    }
}

public enum Measurement {
    public static func profileName(for documentName: String) -> String {
        let stem = (documentName as NSString).deletingPathExtension.trimmingCharacters(in: .whitespaces)
        return stem.isEmpty ? documentName.trimmingCharacters(in: .whitespaces) : stem
    }

    public static func told(_ fit: MeasurementFit, toward target: String) -> String {
        let before = String(format: "%.1f", locale: Locale(identifier: "en_US_POSIX"), fit.offByDbBefore)
        let after = String(format: "%.1f", locale: Locale(identifier: "en_US_POSIX"), fit.offByDbAfter)
        var said = "Fitted \(counted(Int(fit.bands), "band")) toward \(target): off by \(before) dB before, \(after) dB after"
        if fit.imported.passedOver > 0 {
            said += " · \(counted(Int(fit.imported.passedOver), "line")) of the file passed over"
        }
        if fit.imported.clamped > 0 {
            said += " · \(counted(Int(fit.imported.clamped), "value")) held to the limits"
        }
        return said
    }
}
