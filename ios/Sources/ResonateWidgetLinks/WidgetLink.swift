import Foundation

public enum WidgetLink: String, CaseIterable, Sendable {
    case nowPlaying = "now-playing"
    case shuffle
    case search

    private static let scheme = "resonate"
    private static let host = "widget"

    public var url: URL {
        URL(string: "\(Self.scheme)://\(Self.host)/\(rawValue)")!
    }

    public static func recognizes(_ url: URL) -> Bool {
        url.scheme?.lowercased() == scheme && url.host?.lowercased() == host
    }

    public init?(url: URL) {
        guard let match = Self.allCases.first(where: { $0.url == url }) else { return nil }
        self = match
    }
}
