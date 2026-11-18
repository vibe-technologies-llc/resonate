public enum FastScroll {
    public static let appearsFrom = 60
    public static let fadesInOver = 0.15
    public static let lingersFor = 0.9
    public static let fadesOutOver = 0.5
    public static let thumbLength = 52.0
    public static let adjustedBy = 0.1
    public static let movesPast = 4.0

    public static func moved(by distance: Double) -> Bool {
        abs(distance) >= movesPast
    }

    public static func offered(rows: Int) -> Bool {
        rows >= appearsFrom
    }

    public static func scrolled(offset: Double, content: Double, container: Double) -> Double {
        let travel = content - container
        guard travel > 0 else { return 0 }
        return whole(offset / travel)
    }

    public static func share(of window: Range<Int>, in total: Int, scrolled: Double) -> Double {
        guard total > 1 else { return 0 }
        let within = Double(window.lowerBound) + whole(scrolled) * Double(max(window.count - 1, 0))
        return whole(within / Double(total - 1))
    }

    public static func row(at share: Double, of total: Int) -> Int {
        guard total > 1 else { return 0 }
        return Int((whole(share) * Double(total - 1)).rounded())
    }

    public static func thumbOffset(share: Double, track: Double) -> Double {
        whole(share) * max(track - thumbLength, 0)
    }

    public static func share(draggedFrom start: Double, by translation: Double, track: Double) -> Double {
        let travel = track - thumbLength
        guard travel > 0 else { return whole(start) }
        return whole(start + translation / travel)
    }

    public static func adjusted(_ share: Double, forward: Bool) -> Double {
        whole(share + (forward ? adjustedBy : -adjustedBy))
    }

    private static func whole(_ share: Double) -> Double {
        min(max(share, 0), 1)
    }
}
