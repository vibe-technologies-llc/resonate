import Foundation
import ResonateNative

public enum ListeningHistory {
    public static let fileName = "listens.jsonl"

    public static func broughtIn(_ brought: HistoryBroughtIn) -> String {
        said(
            "scrobbling.history.brought-in.detail",
            said("scrobbling.history.brought-in", Int(brought.credited)),
            Int(brought.heldAlready),
            Int(brought.unmatched)
        )
    }
}
