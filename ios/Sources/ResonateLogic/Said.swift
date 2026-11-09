import Foundation

public func said(_ key: String, _ arguments: any CVarArg...) -> String {
    Wording.said(key, arguments)
}

#if canImport(Darwin)
enum Wording {
    static func said(_ key: String, _ arguments: [any CVarArg]) -> String {
        let format = Bundle.main.localizedString(forKey: key, value: nil, table: nil)
        return arguments.isEmpty ? format : String(format: format, locale: .current, arguments: arguments)
    }
}
#else
enum Wording {
    static let english = HostWording(
        folder: URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("Resonate")
            .appendingPathComponent("en.lproj")
    )

    static func said(_ key: String, _ arguments: [any CVarArg]) -> String {
        english.said(key, arguments)
    }
}
#endif
