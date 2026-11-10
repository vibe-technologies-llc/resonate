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
        link.keptInStep ? said("linked.header.kept", link.service) : said("linked.header.imported", link.service)
    }

    public static func keepInStep(_ service: String) -> String {
        said("linked.keep-in-step", service)
    }

    public static func keepingExplained(_ service: String) -> String {
        said("linked.keeping-explained", service)
    }

    public static func refreshFrom(_ service: String) -> String {
        said("linked.refresh-from", service)
    }

    public static func imported(
        name: String,
        service: String,
        keptInStep: Bool,
        matched: UInt32,
        missing: UInt32
    ) -> String {
        guard keptInStep else {
            return said("linked.imported.once", Int(matched), Int(missing))
        }
        return missing == 0
            ? said("linked.imported.kept", name, service, Int(matched))
            : said("linked.imported.kept.downloading", name, service, Int(matched), Int(missing))
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
        if refreshed.added > 0 { parts.append(said("linked.refreshed.added", Int(refreshed.added))) }
        if refreshed.removed > 0 { parts.append(said("linked.refreshed.removed", Int(refreshed.removed))) }
        if asked, refreshed.wanted > 0 {
            parts.append(said("linked.refreshed.downloading", Int(refreshed.wanted)))
        } else if asked, refreshed.missing > 0 {
            parts.append(said("linked.refreshed.missing", Int(refreshed.missing)))
        }

        guard moved(refreshed) || asked else { return nil }
        guard !parts.isEmpty else { return said("linked.refreshed.in-step", name, service) }
        return said("linked.refreshed", name, service, parts.joined(separator: ", "))
    }

    public static func keptInStep(name: String, service: String, _ kept: Bool) -> String {
        kept ? said("linked.keeping", name, service) : said("linked.no-longer-follows", name, service)
    }

    public static func forgotten(name: String, service: String) -> String {
        said("linked.forgotten", name, service)
    }
}

public enum FollowedProfileWords {
    public static func follow(_ service: String) -> String {
        said("linked.profile.follow", service)
    }

    public static var moveOnce: String { said("linked.profile.move-once") }

    public static var reading: String { said("linked.profile.reading") }

    public static func heading(_ service: String) -> String {
        said("linked.profile.heading", service)
    }

    public static func explained(_ service: String) -> String {
        said("linked.profile.explained", service)
    }

    public static func followed(_ profile: FollowedProfile, keptInStep: Bool) -> String {
        let playlists = profile.playlists.count
        var parts: [String] = []
        if keptInStep, profile.letGo > 0 { parts.append(said("linked.profile.let-go", Int(profile.letGo))) }
        if keptInStep, profile.gone > 0 { parts.append(said("linked.profile.deleted", Int(profile.gone), profile.service)) }
        if profile.unread > 0 { parts.append(said("linked.profile.unread", Int(profile.unread))) }

        let lead = keptInStep
            ? said("linked.profile.following", profile.name, profile.service, playlists)
            : said("linked.profile.moved", playlists, profile.name)
        return parts.isEmpty ? lead : lead + ", " + parts.joined(separator: ", ")
    }

    public static func refreshed(name: String, service: String, added: Int, changed: Bool, gone: UInt32) -> String? {
        var parts: [String] = []
        if added > 0 { parts.append(said("linked.profile.new-playlists", added)) }
        if gone > 0 { parts.append(said("linked.profile.deleted", Int(gone), service)) }

        guard parts.isEmpty else { return said("linked.profile.refreshed", name, service, parts.joined(separator: ", ")) }
        return changed ? said("linked.profile.brought-in-step", name, service) : nil
    }

    public static func stopFollowing(_ name: String) -> String {
        said("linked.profile.stop-following.ask", name)
    }

    public static func followed(_ profile: ProfileFollowed) -> String {
        said("linked.profile.on", profile.name, profile.service)
    }

    public static var stopFollowingHeading: String { said("linked.profile.stop-following") }

    public static var stoppingExplained: String { said("linked.profile.stopping-explained") }
}
