import Foundation
import ResonateNative

public enum ListeningHistory {
    static let keptKey = "history-kept-days"

    public static let fileName = "listens.jsonl"

    public static func kept(in settings: UserDefaults) -> KeptHistory {
        let days = settings.integer(forKey: keptKey)
        guard days > 0, let span = UInt16(exactly: days) else { return .forever }
        return .days(days: span)
    }

    public static func keep(_ kept: KeptHistory, in settings: UserDefaults) {
        switch kept {
        case .forever: settings.removeObject(forKey: keptKey)
        case .days(let days): settings.set(Int(days), forKey: keptKey)
        }
    }

    public static func days(of kept: KeptHistory) -> Int? {
        switch kept {
        case .forever: nil
        case .days(let days): Int(days)
        }
    }

    public static func forgetsListens(keeping now: KeptHistory, asked: KeptHistory) -> Bool {
        guard let asking = days(of: asked) else { return false }
        guard let keeping = days(of: now) else { return true }
        return asking < keeping
    }

    public static func label(_ kept: KeptHistory) -> String {
        switch days(of: kept) {
        case nil: said("scrobbling.history.kept.forever")
        case 1826: said("scrobbling.history.kept.five-years")
        case 730: said("scrobbling.history.kept.two-years")
        case 365: said("scrobbling.history.kept.a-year")
        case 182: said("scrobbling.history.kept.six-months")
        case let days?: said("scrobbling.history.kept.days", days)
        }
    }

    public static func broughtIn(_ brought: HistoryBroughtIn) -> String {
        said(
            "scrobbling.history.brought-in.detail",
            said("scrobbling.history.brought-in", Int(brought.credited)),
            Int(brought.heldAlready),
            Int(brought.unmatched)
        )
    }
}
