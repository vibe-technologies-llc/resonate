import ResonateLogic
import ResonateNative
import SwiftUI

enum MixListEntry: Identifiable {
    case local(Track, Int)
    case suggested(ExploreSong, Int)

    var id: String {
        switch self {
        case .local(let track, let index): "local:\(index):\(track.id)"
        case .suggested(let offer, let index): "suggested:\(index):\(offer.song.uri)"
        }
    }
}

func weaveSwiftMix(tracks: [Track], suggestions: [ExploreSong]) -> [MixListEntry] {
    if suggestions.isEmpty {
        return tracks.enumerated().map { .local($0.element, $0.offset) }
    }
    if tracks.isEmpty {
        return suggestions.enumerated().map { .suggested($0.element, $0.offset) }
    }
    let cadence = tracks.count >= 7 ? 3 : (tracks.count >= 4 ? 2 : 1)
    var remaining = suggestions
    var woven: [MixListEntry] = []
    woven.reserveCapacity(tracks.count + suggestions.count)
    for (index, track) in tracks.enumerated() {
        woven.append(.local(track, index))
        if (index + 1) % cadence == 0, !remaining.isEmpty {
            let prevArtist = track.artist.trimmingCharacters(in: .whitespaces).lowercased()
            let nextArtist = tracks.indices.contains(index + 1)
                ? tracks[index + 1].artist.trimmingCharacters(in: .whitespaces).lowercased()
                : nil
            let ideal = remaining.firstIndex { offer in
                let artist = (offer.song.artist ?? "").trimmingCharacters(in: .whitespaces).lowercased()
                return artist != prevArtist && artist != nextArtist
            }
            let chosen = remaining.remove(at: ideal ?? 0)
            woven.append(.suggested(chosen, woven.count))
        }
    }
    return woven
}

extension ExplorePlaylistKind {
    var symbol: String {
        switch self {
        case .discoverMix: "sparkles"
        case .beyondArtist: "music.mic"
        case .genreDiscovery: "guitars"
        case .favouritesExpansion: "heart.fill"
        case .rediscoverBeyond: "opticaldisc"
        }
    }
}

struct ExplorePlaylistsSection: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let columns: [GridItem]
    @State private var loaded: Loaded?
    @State private var retry = 0
    @State private var retried = 0

    private struct Request: Equatable {
        let services: Int
        let catalog: Int
        let day: Date
        let online: Bool
        let retry: Int
    }

    private struct Loaded {
        let request: Request
        let feed: ExplorePlaylistsFeed?
    }

    var body: some View {
        let now = Date()
        let request = Request(
            services: library.features.servedGeneration,
            catalog: library.catalogEpoch,
            day: Calendar.current.startOfDay(for: now),
            online: library.features.online,
            retry: retry
        )
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("explore.title")
                    .font(.title3.weight(.semibold))
                    .accessibilityAddTraits(.isHeader)
                Spacer(minLength: 0)
                if request.online {
                    Button("explore.refresh", systemImage: "arrow.clockwise") { retry += 1 }
                        .labelStyle(.iconOnly)
                        .buttonStyle(.borderless)
                }
            }
            if !request.online {
                Text("explore.offline").foregroundStyle(.secondary)
            } else if let loaded,
                      loaded.request.online == request.online,
                      loaded.request.services == request.services,
                      loaded.request.retry == request.retry {
                if let feed = loaded.feed {
                    if feed.services == 0 {
                        Text("explore.services").foregroundStyle(.secondary)
                    } else {
                        if !feed.playlists.isEmpty {
                            LazyVGrid(columns: columns, spacing: 14) {
                                ForEach(feed.playlists, id: \.id) { playlist in
                                    ExplorePlaylistCard(playlist: playlist) {
                                        navigation.open(.explorePlaylist(playlist.id, playlist.name))
                                    }
                                }
                            }
                        }
                        if feed.failed > 0 {
                            failure
                        } else if feed.playlists.isEmpty {
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
            let found = try? await library.features.serverAnswer {
                try $0.explorePlaylists(timeZone: identifier, utcOffsetSeconds: offset, refresh: refresh)
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
}

private struct ExplorePlaylistCard: View {
    let playlist: ExplorePlaylist
    let open: () -> Void

    var body: some View {
        Button(action: open) {
            VStack(alignment: .leading, spacing: 8) {
                ZStack(alignment: .bottomLeading) {
                    RoundedRectangle(cornerRadius: 16, style: .continuous)
                        .fill(Color.cardGround)
                        .aspectRatio(1, contentMode: .fit)
                    VStack(alignment: .leading, spacing: 6) {
                        Image(systemName: playlist.kind.symbol)
                            .font(.title2)
                            .foregroundStyle(.tint)
                            .accessibilityHidden(true)
                        Spacer(minLength: 0)
                        Text(playlist.name)
                            .font(.headline)
                            .lineLimit(2)
                    }
                    .padding(14)
                }
                Text(playlist.detail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .buttonStyle(.plain)
    }
}

struct ExplorePlaylistView: View {
    @Environment(Library.self) private var library
    let id: String
    let fallbackName: String
    @State private var playlist: ExplorePlaylist?
    @State private var loaded = false
    @State private var retry = 0

    var body: some View {
        let downloads = library.features.downloads
        Group {
            if let playlist {
                List {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(playlist.name).font(.title2.weight(.bold))
                        Text(playlist.detail).font(.subheadline).foregroundStyle(.secondary)
                        HStack(spacing: 12) {
                            Button("common.play", systemImage: "play.fill") {
                                library.playServed(playlist.songs.map(\.song.uri), at: 0)
                            }
                            .buttonStyle(.borderedProminent)
                            Button("common.shuffle", systemImage: "shuffle") {
                                library.shuffleServed(playlist.songs.map(\.song.uri))
                            }
                            .buttonStyle(.bordered)
                            Button("explore.download-all", systemImage: "arrow.down.to.line") {
                                downloads.enqueue(playlist.songs.map(\.download))
                            }
                            .labelStyle(.iconOnly)
                            .buttonStyle(.bordered)
                        }
                        .padding(.top, 4)
                    }
                    .listRowBackground(Color.clear)
                    ForEach(Array(playlist.songs.enumerated()), id: \.element.song.uri) { index, offer in
                        let song = offer.song
                        HStack(spacing: 12) {
                            Button {
                                library.playServed(playlist.songs.map(\.song.uri), at: index)
                            } label: {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(song.title).titleLines()
                                    if let artist = song.artist {
                                        Text(artist).font(.subheadline).foregroundStyle(.secondary)
                                    }
                                    Text(offer.service).font(.caption).foregroundStyle(.secondary)
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                            }
                            .buttonStyle(.plain)
                            ExploreDownloadAction(title: song.title, held: downloads.holds(offer.download)) {
                                downloads.enqueue([offer.download])
                            }
                        }
                        .listRowBackground(Color.clear)
                    }
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                .navigationTitle(playlist.name)
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .topBarTrailing) {
                        Button("explore.refresh", systemImage: "arrow.clockwise") { retry += 1 }
                    }
                    ToolbarItem(placement: .topBarTrailing) { DownloadsButton() }
                }
                .downloadDestinationPrompt()
            } else if loaded {
                ContentUnavailableView("explore.playlist-gone", systemImage: "sparkles")
            } else {
                ProgressView("explore.loading")
                    .navigationTitle(fallbackName)
            }
        }
        .task(id: retry) {
            let now = Date()
            let zone = TimeZone.current
            let identifier = zone.identifier
            let offset = Int32(zone.secondsFromGMT(for: now))
            let refresh = retry > 0
            let found = try? await library.features.serverAnswer {
                try $0.explorePlaylists(timeZone: identifier, utcOffsetSeconds: offset, refresh: refresh)
            }
            if !Task.isCancelled {
                playlist = found?.playlists.first { $0.id == id }
                loaded = true
            }
        }
    }
}

struct ExploreWovenRow: View {
    @Environment(Library.self) private var library
    let offer: ExploreSong
    let onPlay: () -> Void
    let onDismiss: () -> Void

    var body: some View {
        let song = offer.song
        let downloads = library.features.downloads
        let held = downloads.holds(offer.download)
        HStack(spacing: 12) {
            Button(action: onPlay) {
                HStack(spacing: 12) {
                    Image(systemName: "sparkles")
                        .foregroundStyle(.tint)
                        .frame(width: 44, height: 44)
                        .background(Color.cardGround, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                        .accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(song.title).titleLines()
                        if let artist = song.artist {
                            Text(artist).font(.subheadline).foregroundStyle(.secondary)
                        }
                        Text(said("explore.suggested", offer.service))
                            .font(.caption)
                            .foregroundStyle(.tint)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .buttonStyle(.plain)
            ExploreDownloadAction(title: song.title, held: held) {
                downloads.enqueue([offer.download])
            }
            Button(said("explore.dismiss", song.title), systemImage: "xmark") {
                onDismiss()
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
        }
    }
}

private struct ExploreDownloadAction: View {
    let title: String
    let held: Bool
    let action: () -> Void

    var body: some View {
        if held {
            Button(said("explore.downloaded", title), systemImage: "checkmark", action: action)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .disabled(true)
        } else {
            Button(said("explore.download", title), systemImage: "arrow.down.to.line", action: action)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
        }
    }
}

struct ExploreDownloadButton: View {
    @Environment(Library.self) private var library
    let uri: String
    @State private var offer: ExploreSong?

    var body: some View {
        Group {
            if let offer {
                let downloads = library.features.downloads
                ExploreDownloadAction(title: offer.song.title, held: downloads.holds(offer.download)) {
                    downloads.enqueue([offer.download])
                }
            }
        }
        .task(id: uri) {
            offer = await library.exploreDownload(for: uri)
        }
    }
}
