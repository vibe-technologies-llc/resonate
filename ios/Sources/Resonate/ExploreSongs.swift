import ResonateLogic
import ResonateNative
import SwiftUI

struct ExploreSongs: View {
    @Environment(Library.self) private var library
    let scope: ExploreScope
    @State private var loaded: Loaded?
    @State private var retry = 0
    @State private var retried = 0

    private struct Request: Equatable {
        let scope: ExploreScope
        let services: Int
        let catalog: Int
        let collections: Int
        let day: Date
        let online: Bool
        let retry: Int
    }

    private struct Loaded {
        let request: Request
        let feed: ExploreFeed?
    }

    var body: some View {
        let now = Date()
        let request = Request(
            scope: scope,
            services: library.features.servedGeneration,
            catalog: library.catalogEpoch,
            collections: library.collectionGeneration,
            day: Calendar.current.startOfDay(for: now),
            online: library.features.online,
            retry: retry
        )
        Group {
            if !request.online {
                Text("explore.offline").foregroundStyle(.secondary)
            } else if let loaded, loaded.request == request {
                if let feed = loaded.feed {
                    if feed.services == 0 {
                        Text("explore.services").foregroundStyle(.secondary)
                    } else {
                        ForEach(feed.songs, id: \.song.uri) { offer in row(offer) }
                        if feed.failed > 0 {
                            failure
                        } else if feed.songs.isEmpty {
                            Text("explore.empty").foregroundStyle(.secondary)
                        }
                    }
                } else {
                    failure
                }
            } else {
                ProgressView("explore.loading")
            }
        }
        .task(id: request) {
            guard request.online else { return }
            let refresh = request.retry != retried
            retried = request.retry
            let zone = TimeZone.current
            let identifier = zone.identifier
            let offset = Int32(zone.secondsFromGMT(for: now))
            let scope = scope
            let found = try? await library.features.serverAnswer {
                try $0.explore(scope: scope, timeZone: identifier, utcOffsetSeconds: offset, refresh: refresh)
            }
            if !Task.isCancelled { loaded = Loaded(request: request, feed: found) }
        }
    }

    private var failure: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("explore.failed").foregroundStyle(.secondary)
            Button("explore.retry") { retry += 1 }
                .buttonStyle(.borderless)
        }
    }

    private func row(_ offer: ExploreSong) -> some View {
        let song = offer.song
        let downloads = library.features.downloads
        return HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(song.title).titleLines()
                if let artist = song.artist {
                    Text(artist).font(.subheadline).foregroundStyle(.secondary)
                }
                Text(offer.service).font(.caption).foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
            Button(said("explore.play", song.title), systemImage: "play.fill") {
                library.playServed([song.uri], at: 0)
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            Button(said("explore.download", song.title), systemImage: "arrow.down.to.line") {
                downloads.enqueue([offer.download])
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            .disabled(downloads.holds(offer.download))
        }
    }
}
