import ResonateNative

public struct OutputRoute: Equatable, Sendable {
    public let id: String
    public let name: String

    public init(id: String, name: String) {
        self.id = id
        self.name = name
    }
}


extension GainMode {
    public static let every: [GainMode] = [.off, .track, .album]
    public var name: String { "\(self)" }
    public static func named(_ name: String?) -> GainMode? { every.first { $0.name == name } }
}

extension ResamplingQuality {
    public static let every: [ResamplingQuality] = [.lowLatency, .balanced, .high, .veryHigh]
    public var name: String { "\(self)" }
    public static func named(_ name: String?) -> ResamplingQuality? {
        name == "fast" ? .lowLatency : every.first { $0.name == name }
    }
}

extension ResamplingPhase {
    public static let every: [ResamplingPhase] = [.linear, .intermediate, .minimum]
    public var name: String { "\(self)" }
    public static func named(_ name: String?) -> ResamplingPhase? { every.first { $0.name == name } }
}

extension CrossfeedLevel {
    public static let every: [CrossfeedLevel] = [.off, .light, .moderate, .strong]
    public var name: String { "\(self)" }
    public static func named(_ name: String?) -> CrossfeedLevel? { every.first { $0.name == name } }
}

extension ShuffleStyle {
    public static let every: [ShuffleStyle] = [.random, .smart, .albums]
    public var name: String { "\(self)" }
    public static func named(_ name: String?) -> ShuffleStyle? { every.first { $0.name == name } }
}

extension LossyRestoration {
    public static let every: [LossyRestoration] = [.off, .repair, .extend]
    public var name: String { "\(self)" }
    public static func named(_ name: String?) -> LossyRestoration? { every.first { $0.name == name } }
}

extension MusicProvider {
    public static let every: [MusicProvider] = [.subsonic, .jellyfin, .tidal, .hifiApi, .monochrome]
    public var name: String { "\(self)" }
    public static func named(_ name: String?) -> MusicProvider? { every.first { $0.name == name } }
}
