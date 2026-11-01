import Foundation
import ResonateNative

public enum LinkedPlaylistPace {
    public static let firstLook: Duration = .seconds(60)
    public static let lookEvery: Duration = .seconds(600)

    public static func follows(online: Bool, foreground: Bool) -> Bool {
        online && foreground
    }
}

public enum LinkedPlaylistWords {
    public static func header(_ link: PlaylistLinkInfo) -> String {
        link.keptInStep ? "Kept in step with \(link.service)" : "Imported from \(link.service)"
    }

    public static func keepInStep(_ service: String) -> String {
        "Keep in step with \(service)"
    }

    public static func keepingExplained(_ service: String) -> String {
        "The playlist follows the one on \(service), read again every few hours: songs in your library take its order, and the rest are saved for later to download."
    }

    public static func refreshFrom(_ service: String) -> String {
        "Refresh from \(service)"
    }

    public static func imported(
        name: String,
        service: String,
        keptInStep: Bool,
        matched: UInt32,
        missing: UInt32
    ) -> String {
        guard keptInStep else {
            return "Imported \(counted(Int(matched), "local song")); \(missing) still missing"
        }
        let here = "\(counted(Int(matched), "song")) here"
        let following = "Keeping \(name) in step with \(service)"
        return missing == 0 ? "\(following) · \(here)" : "\(following) · \(here), \(missing) being downloaded"
    }

    public static func moved(_ refreshed: RefreshedPlaylist) -> Bool {
        refreshed.added > 0 || refreshed.removed > 0
    }

    public static func refreshed(
        name: String,
        service: String,
        _ refreshed: RefreshedPlaylist,
        asked: Bool
    ) -> String? {
        var parts: [String] = []
        if refreshed.added > 0 { parts.append("\(counted(Int(refreshed.added), "song")) added") }
        if refreshed.removed > 0 { parts.append("\(refreshed.removed) taken out") }
        if asked, refreshed.wanted > 0 {
            parts.append("\(refreshed.wanted) being downloaded")
        } else if asked, refreshed.missing > 0 {
            parts.append("\(refreshed.missing) not in your library")
        }

        guard moved(refreshed) || asked else { return nil }
        guard !parts.isEmpty else { return "\(name) is in step with \(service)" }
        return "\(name) from \(service) · " + parts.joined(separator: ", ")
    }

    public static func keptInStep(name: String, service: String, _ kept: Bool) -> String {
        kept ? "Keeping \(name) in step with \(service)" : "\(name) no longer follows \(service)"
    }

    public static func forgotten(name: String, service: String) -> String {
        "\(name) no longer remembers its \(service) link"
    }
}

public enum FollowedProfileWords {
    public static func follow(_ service: String) -> String {
        "Follow \(service) profile"
    }

    public static let moveOnce = "Move to Resonate once"

    public static let reading = "Reading the profile…"

    public static func heading(_ service: String) -> String {
        "\(service) profile"
    }

    public static func explained(_ service: String) -> String {
        "Following keeps every playlist this profile makes, and its loved tracks, in step with \(service), new playlists included. Moving once imports them as they are now and follows nothing."
    }

    public static func followed(_ profile: FollowedProfile, keptInStep: Bool) -> String {
        let playlists = counted(profile.playlists.count, "playlist")
        var parts: [String] = []
        if keptInStep, profile.letGo > 0 { parts.append("\(profile.letGo) let go") }
        if keptInStep, profile.gone > 0 { parts.append("\(profile.gone) deleted on \(profile.service)") }
        if profile.unread > 0 { parts.append("\(profile.unread) could not be read") }

        let lead = keptInStep
            ? "Following \(profile.name) on \(profile.service) · \(playlists)"
            : "Moved \(playlists) of \(profile.name) to Resonate"
        return parts.isEmpty ? lead : lead + ", " + parts.joined(separator: ", ")
    }

    public static func refreshed(name: String, service: String, added: Int, changed: Bool, gone: UInt32) -> String? {
        var parts: [String] = []
        if added > 0 { parts.append(counted(added, "new playlist")) }
        if gone > 0 { parts.append("\(gone) deleted on \(service)") }

        guard parts.isEmpty else { return "\(name) on \(service) · " + parts.joined(separator: ", ") }
        return changed ? "\(name)'s playlists on \(service) brought in step" : nil
    }

    public static func stopFollowing(_ name: String) -> String {
        "Stop following \(name)?"
    }

    public static func followed(_ profile: ProfileFollowed) -> String {
        "\(profile.name) on \(profile.service)"
    }

    public static let stopFollowingHeading = "Stop following"

    public static let stoppingExplained = "Its playlists stay, still kept in step one by one; new playlists it makes are no longer brought in."
}
