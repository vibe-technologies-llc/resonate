import Foundation

public enum HoldEffect: Equatable, Sendable {
    case beginPlain
    case endPlain
    case submit(String)
    case forgetAsked(String)
    case complete(success: Bool)
    case notifyExpired
    case show
}

public struct HoldMachine: Sendable {
    private var plain = false
    public private(set) var request: String?
    public private(set) var continuing = false
    private let continues: Bool

    public init(continues: Bool) {
        self.continues = continues
    }

    public var holdingPlain: Bool { plain }

    public mutating func hold(_ busy: Bool, active: Bool, identifier: () -> String) -> [HoldEffect] {
        guard busy else { return endingPlain() + finishing(success: true) }
        var effects: [HoldEffect] = []
        if !plain, !continuing {
            plain = true
            effects.append(.beginPlain)
        }
        guard continues, request == nil, !continuing, active else { return effects }
        effects.append(.submit(identifier()))
        return effects
    }

    public mutating func submitted(_ identifier: String, accepted: Bool) {
        if accepted { request = identifier }
    }

    public func accepts(_ identifier: String) -> Bool {
        request == identifier
    }

    public mutating func began() -> [HoldEffect] {
        request = nil
        continuing = true
        return endingPlain() + [.show]
    }

    public mutating func expired() -> [HoldEffect] {
        [.notifyExpired] + endingPlain() + finishing(success: false)
    }

    public func shows() -> Bool {
        continuing
    }

    private mutating func endingPlain() -> [HoldEffect] {
        guard plain else { return [] }
        plain = false
        return [.endPlain]
    }

    private mutating func finishing(success: Bool) -> [HoldEffect] {
        var effects: [HoldEffect] = []
        if let request { effects.append(.forgetAsked(request)) }
        request = nil
        if continuing {
            effects.append(.complete(success: success))
            continuing = false
        }
        return effects
    }
}
