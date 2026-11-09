import ResonateLogic
import ResonateNative
import SwiftUI

struct StatisticsView: View {
    @Environment(Library.self) private var library
    @State private var days: UInt32 = 0
    @State private var widened: Set<StatisticsRank> = []
    @State private var showingWrapped = false

    private static var spans: [(UInt32, String)] {
        [(7, said("statistics.span.week")), (30, said("statistics.span.month")), (365, said("statistics.span.year")), (0, said("statistics.span.all-time"))]
    }
    private static let shortList = 5
    private static let longList = 25

    var body: some View {
        let features = library.features
        let statistics = features.statistics
        let detail = features.listeningDetail
        let loadedDays = features.statisticsDays ?? days
        ThemedList {
            Picker("statistics.period", selection: $days) {
                ForEach(Self.spans, id: \.0) { Text($0.1).tag($0.0) }
            }
            .pickerStyle(.segmented)
            if let statistics, let detail {
                Section {
                    StatisticsLedger(statistics: statistics, detail: detail, span: loadedDays)
                        .listRowInsets(EdgeInsets())
                }
                if statistics.plays == 0 {
                    ContentUnavailableView(
                        "statistics.no-plays",
                        systemImage: "chart.bar",
                        description: Text("statistics.no-plays.detail")
                    )
                    .listRowBackground(Color.clear)
                } else {
                    if !detail.days.isEmpty {
                        HeadedSection(said("statistics.over-time")) {
                            ListeningChart(bars: features.listeningBars, span: loadedDays)
                                .padding(.vertical, 8)
                        }
                    }
                    songs(statistics.topTracks)
                    albums(detail.albums)
                    artists(detail.artists)
                }
                if let breakdown = features.libraryBreakdown {
                    LibraryBreakdownSections(breakdown: breakdown, songs: statistics.songs)
                }
            }
        }
        .overlay {
            if statistics == nil, features.statisticsLoading {
                ProgressView("statistics.reading.long")
            }
        }
        .toolbar {
            if statistics != nil, features.statisticsLoading {
                ProgressView().accessibilityLabel("statistics.reading")
            }
            if features.wrappedYear != nil {
                Button("statistics.wrapped", systemImage: "rectangle.stack") { showingWrapped = true }
            }
        }
        .navigationTitle("tools.statistics")
        .refreshable { features.loadStatistics(days: days) }
        .task(id: days) { features.loadStatistics(days: days) }
        .task { features.loadWrapped() }
        .fullScreenCover(isPresented: $showingWrapped) {
            if let year = features.wrappedYear {
                WrappedDeck(year: year)
            }
        }
    }

    private func shown<Row>(_ rows: [Row], _ rank: StatisticsRank) -> [Row] {
        Array(rows.prefix(widened.contains(rank) ? Self.longList : Self.shortList))
    }

    private func more(_ count: Int, _ rank: StatisticsRank) -> some View {
        Group {
            if count > Self.shortList {
                Button(said(widened.contains(rank) ? "statistics.show-fewer" : "statistics.show-more")) {
                    if !widened.insert(rank).inserted { widened.remove(rank) }
                }
            }
        }
    }

    @ViewBuilder
    private func songs(_ tracks: [Track]) -> some View {
        if !tracks.isEmpty {
            HeadedSection(said("statistics.top.songs")) {
                ForEach(shown(tracks, .songs), id: \.id) { track in
                    TrackRow(track: track) { library.play(tracks, selected: track) }
                }
                more(tracks.count, .songs)
            }
        }
    }

    @ViewBuilder
    private func albums(_ albums: [ListenedTo]) -> some View {
        if !albums.isEmpty {
            HeadedSection(said("statistics.top.albums")) {
                ForEach(shown(albums, .albums), id: \.key) { album in
                    NavigationLink(value: Destination.album(album.key)) {
                        CollectionRow(track: cover(of: album, in: \.albums), title: album.name, detail: album.heard, measure: .song)
                    }
                    .collectionActions(.album(album.key), artist: album.by)
                }
                more(albums.count, .albums)
            }
        }
    }

    @ViewBuilder
    private func artists(_ artists: [ListenedTo]) -> some View {
        if !artists.isEmpty {
            HeadedSection(said("statistics.top.artists")) {
                ForEach(shown(artists, .artists), id: \.key) { artist in
                    NavigationLink(value: Destination.artist(artist.name)) {
                        CollectionRow(
                            track: cover(of: artist, in: \.artists),
                            title: artist.name,
                            detail: artist.heard,
                            circle: true,
                            measure: .song
                        )
                    }
                    .collectionActions(.artist(artist.name))
                }
                more(artists.count, .artists)
            }
        }
    }

    private func cover(of listened: ListenedTo, in kind: KeyPath<ListenedCovers, [String: UInt64]>) -> Track? {
        library.features.listenedCovers[keyPath: kind][listened.key].flatMap(library.artworkTrack)
    }

    nonisolated static func spoken(_ milliseconds: UInt64) -> String {
        ListenedTime.spoken(milliseconds)
    }

    static func brief(_ milliseconds: UInt64) -> String {
        ListenedTime.brief(milliseconds)
    }
}

enum StatisticsRank: Hashable {
    case songs, albums, artists
}

struct Ledger: View {
    struct Cell {
        let title: String
        let value: String
        let detail: String?
    }

    private static let acrossWhenWide = 3
    private static let acrossWhenNarrow = 2

    @Environment(\.dynamicTypeSize) private var typeSize
    @Environment(\.horizontalSizeClass) private var horizontal
    let cells: [Cell]

    var body: some View {
        let across = typeSize.isAccessibilitySize ? 1 : horizontal == .regular ? Self.acrossWhenWide : Self.acrossWhenNarrow
        let lines = stride(from: 0, to: cells.count, by: across).map { Array(cells[$0..<min($0 + across, cells.count)]) }
        VStack(spacing: 0) {
            ForEach(lines.indices, id: \.self) { line in
                if line > 0 { Divider() }
                HStack(spacing: 0) {
                    ForEach(0..<across, id: \.self) { column in
                        if column > 0 { Divider() }
                        if column < lines[line].count {
                            LedgerCell(cell: lines[line][column])
                        } else {
                            Color.clear.frame(maxWidth: .infinity)
                        }
                    }
                }
                .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

private struct LedgerCell: View {
    let cell: Ledger.Cell

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(cell.title)
                .font(.caption.weight(.medium))
                .foregroundStyle(.secondary)
                .titleLines()
            Text(cell.value)
                .font(.title3.weight(.semibold))
                .minimumScaleFactor(0.7)
                .titleLines()
            if let detail = cell.detail {
                Text(detail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .titleLines()
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(cell.title)
        .accessibilityValue(cell.detail.map { "\(cell.value), \($0)" } ?? cell.value)
    }
}

struct StatisticsLedger: View {
    let statistics: LibraryStatistics
    let detail: ListeningDetail
    let span: UInt32

    var body: some View {
        let busiest = detail.days.max { $0.listenedMs < $1.listenedMs }
        Ledger(cells: [
            Ledger.Cell(
                title: said("statistics.listened"),
                value: StatisticsView.brief(statistics.listenedMs),
                detail: said("counted.plays", Int(statistics.plays))
            ),
            Ledger.Cell(
                title: said("statistics.daily-average"),
                value: StatisticsView.brief(average),
                detail: span == 0 ? said("statistics.daily-average.since-first") : said("statistics.daily-average.over-days", Int(span))
            ),
            Ledger.Cell(
                title: said("statistics.songs-heard"),
                value: "\(detail.songsHeard)",
                detail: said("statistics.songs-heard.of", Int(statistics.songs))
            ),
            Ledger.Cell(
                title: said("statistics.busiest-day"),
                value: busiest.map { StatisticsView.brief($0.listenedMs) } ?? said("statistics.busiest-day.none"),
                detail: busiest.map { Date(timeIntervalSince1970: TimeInterval($0.starts)).formatted(span == 0 || span == 365 ? .dateTime.day().month(.abbreviated).year() : .dateTime.day().month(.abbreviated)) }
            ),
            Ledger.Cell(
                title: said("library.title"),
                value: said("counted.songs", Int(statistics.songs)),
                detail: StatisticsView.brief(statistics.durationMs)
            ),
            Ledger.Cell(
                title: said("statistics.albums-and-artists"),
                value: "\(statistics.albums) · \(statistics.artists)",
                detail: said("statistics.albums-and-artists.detail")
            ),
        ])
    }

    private var average: UInt64 {
        let days = max(daysCovered, 1)
        return statistics.listenedMs / days
    }

    private var daysCovered: UInt64 {
        if span != 0 { return UInt64(span) }
        guard let first = detail.firstHeard else { return 1 }
        let seconds = max(Date.now.timeIntervalSince1970 - TimeInterval(first), 0)
        return UInt64((seconds / 86_400).rounded(.up))
    }
}

extension ListenedTo {
    var heard: String {
        let heard = "\(said("counted.plays", Int(plays))) · \(StatisticsView.spoken(listenedMs))"
        return by.isEmpty ? heard : "\(by) · \(heard)"
    }
}

struct ListenedCovers: Equatable, Sendable {
    var albums: [String: UInt64] = [:]
    var artists: [String: UInt64] = [:]

    nonisolated static func of(_ detail: ListeningDetail, from native: MobilePlayer) -> ListenedCovers {
        var covers = ListenedCovers()
        for album in detail.albums {
            if let hit = try? native.album(key: album.key) { covers.albums[album.key] = hit.artworkTrack }
        }
        for artist in detail.artists {
            if let played = try? native.mostPlayedBy(name: artist.name, limit: 1).first { covers.artists[artist.key] = played.id }
        }
        return covers
    }
}

struct ListeningChart: View {
    let bars: [ListeningBar]
    let span: UInt32
    @State private var selected: Date?

    var body: some View {
        ListeningBars(bars: bars, span: span, tallest: max(bars.map(\.listenedMs).max() ?? 1, 1), selected: $selected)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("statistics.over-time")
            .accessibilityValue(spoken(bars))
            .accessibilityHint("statistics.chart.hint")
            .accessibilityAdjustableAction { direction in
                step(direction, through: bars)
            }
            .onChange(of: span) { selected = nil }
    }

    private func spoken(_ bars: [ListeningBar]) -> String {
        guard let chosen = bars.first(where: { $0.starts == selected }) else { return summary(bars) }
        return "\(chosen.caption(span: span)), \(StatisticsView.spoken(chosen.listenedMs)), \(said("counted.plays", Int(chosen.plays)))"
    }

    private func step(_ direction: AccessibilityAdjustmentDirection, through bars: [ListeningBar]) {
        guard !bars.isEmpty else { return }
        let at = bars.firstIndex { $0.starts == selected }
        let next: Int
        switch direction {
        case .increment: next = at.map { min($0 + 1, bars.count - 1) } ?? 0
        case .decrement: next = at.map { max($0 - 1, 0) } ?? bars.count - 1
        @unknown default: return
        }
        selected = bars[next].starts
    }

    private func summary(_ bars: [ListeningBar]) -> String {
        guard let busiest = bars.max(by: { $0.listenedMs < $1.listenedMs }) else { return said("statistics.chart.nothing-heard") }
        return said("statistics.chart.summary", bars.count, StatisticsView.spoken(busiest.listenedMs), busiest.caption(span: span))
    }
}

struct ListeningBars: View {
    let bars: [ListeningBar]
    let span: UInt32
    let tallest: UInt64
    @Binding var selected: Date?
    @ScaledMetric(relativeTo: .body) private var height: CGFloat = 120

    var body: some View {
        let chosen = bars.first { $0.starts == selected }
        VStack(alignment: .leading, spacing: 8) {
            readout(chosen, bars)
            GeometryReader { proxy in
                HStack(alignment: .bottom, spacing: bars.count > 40 ? 1 : 3) {
                    ForEach(bars) { bar in
                        RoundedRectangle(cornerRadius: 2, style: .continuous)
                            .fill(Color.worn.opacity(chosen == nil || chosen == bar ? 1 : 0.35))
                            .frame(height: max(height * CGFloat(bar.listenedMs) / CGFloat(tallest), 2))
                            .frame(maxWidth: .infinity)
                    }
                }
                .frame(maxHeight: .infinity, alignment: .bottom)
                .contentShape(Rectangle())
                .gesture(SpatialTapGesture().onEnded { choose(at: $0.location.x, of: proxy.size.width, in: bars) })
                .simultaneousGesture(
                    DragGesture(minimumDistance: 12)
                        .onChanged { choose(at: $0.location.x, of: proxy.size.width, in: bars) }
                )
            }
            .frame(height: height)
            HStack {
                Text(bars.first?.starts ?? .now, format: ListeningBar.edgeFormat(for: span))
                Spacer()
                Text(bars.last?.starts ?? .now, format: ListeningBar.edgeFormat(for: span))
            }
            .font(.caption)
            .foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private func readout(_ chosen: ListeningBar?, _ bars: [ListeningBar]) -> some View {
        if let chosen {
            VStack(alignment: .leading, spacing: 2) {
                Text(StatisticsView.spoken(chosen.listenedMs)).font(.headline)
                Text("\(chosen.caption(span: span)) · \(said("counted.plays", Int(chosen.plays)))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        } else {
            VStack(alignment: .leading, spacing: 2) {
                Text(StatisticsView.spoken(bars.reduce(0) { $0 + $1.listenedMs })).font(.headline)
                Text(touchHint)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func choose(at x: CGFloat, of width: CGFloat, in bars: [ListeningBar]) {
        guard !bars.isEmpty, width > 0 else { return }
        let place = Int(x / width * CGFloat(bars.count))
        let starts = bars[min(max(place, 0), bars.count - 1)].starts

        guard selected != starts else { return }
        selected = starts
    }

    private var touchHint: String {
        switch span {
        case 0: said("statistics.chart.touch.month")
        case 365: said("statistics.chart.touch.week")
        default: said("statistics.chart.touch.day")
        }
    }
}

struct ShareBars: View {
    let shares: [LibraryShare]
    let songs: UInt64

    var body: some View {
        ForEach(shares, id: \.name) { share in
            let fraction = songs == 0 ? 0 : min(Double(share.songs) / Double(songs), 1)
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text(share.name)
                    Spacer()
                    Text("\(said("counted.songs", Int(share.songs))) · \(fraction.formatted(.percent.precision(.fractionLength(0))))")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                GeometryReader { proxy in
                    Capsule()
                        .fill(Color.worn.opacity(0.18))
                        .overlay(alignment: .leading) {
                            Capsule()
                                .fill(Color.worn)
                                .frame(width: max(proxy.size.width * fraction, 4))
                        }
                }
                .frame(height: 6)
            }
            .padding(.vertical, 2)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(share.name)
            .accessibilityValue("\(said("counted.songs", Int(share.songs))), \(fraction.formatted(.percent.precision(.fractionLength(0))))")
        }
    }
}

struct LibraryBreakdownSections: View {
    let breakdown: LibraryBreakdown
    let songs: UInt64

    var body: some View {
        if !breakdown.formats.isEmpty {
            HeadedSection(said("statistics.breakdown.formats")) { ShareBars(shares: breakdown.formats, songs: songs) }
        }
        if !breakdown.rates.isEmpty {
            HeadedSection(said("statistics.breakdown.rates")) { ShareBars(shares: breakdown.rates, songs: songs) }
        }
        if !breakdown.genres.isEmpty {
            HeadedSection(said("statistics.breakdown.genres")) { ShareBars(shares: breakdown.genres, songs: songs) }
        }
        if !breakdown.decades.isEmpty {
            HeadedSection(said("statistics.breakdown.decades")) { ShareBars(shares: breakdown.decades, songs: songs) }
        }
    }
}
