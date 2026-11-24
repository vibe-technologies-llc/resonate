import Foundation

public enum DocumentPresence: Sendable, Equatable {
    case standing
    case absent
    case unknown

    public static func of(_ file: URL, under root: URL) -> DocumentPresence {
        of(file, under: root) { try? FileManager.default.contentsOfDirectory(atPath: $0.path) }
    }

    public static func of(_ file: URL, under root: URL, listing: (URL) -> [String]?) -> DocumentPresence {
        let top = root.standardizedFileURL.pathComponents
        let whole = file.standardizedFileURL.pathComponents
        guard whole.count > top.count, Array(whole.prefix(top.count)) == top else { return .unknown }
        let steps = Array(whole.dropFirst(top.count))
        return presence(of: steps, at: steps.count - 1, under: root, listing: listing)
    }

    private static func presence(
        of steps: [String],
        at depth: Int,
        under root: URL,
        listing: (URL) -> [String]?
    ) -> DocumentPresence {
        let folder = steps.prefix(depth).reduce(root) { $0.appendingPathComponent($1) }
        if let names = listing(folder) { return names.contains(steps[depth]) ? .standing : .absent }
        if depth == 0 { return .unknown }
        return presence(of: steps, at: depth - 1, under: root, listing: listing) == .absent ? .absent : .unknown
    }
}

public enum OpenFailure: Sendable, Equatable {
    case notFound
    case refused

    public static func after(_ failure: any Swift.Error, presence: () -> DocumentPresence) -> OpenFailure {
        FileRemoval.saysNoSuchFile(failure as NSError) && presence() == .absent ? .notFound : .refused
    }

    public static func opening(_ file: URL, under root: URL, failedWith code: Int32) -> OpenFailure {
        after(POSIXError(POSIXErrorCode(rawValue: code) ?? .EIO)) { DocumentPresence.of(file, under: root) }
    }
}
