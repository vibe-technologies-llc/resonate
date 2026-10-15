import ResonateNative
import SwiftUI

struct StatisticsView: View {
    @Environment(Library.self) private var library
    @State private var days: UInt32 = 0
    @State private var selected: Date?
    @State private var widened: Set<StatisticsRank> = []

    private static let spans: [(UInt32, String)] = [(7, "Week"), (30, "Month"), (365, "Year"), (0, "All time")]
    private static let shortList = 5
    private static let longList = 25

    var body: some View {
        let features = library.features
        let statistics = features.statistics
        let detail = features.listeningDetail
        let loadedDays = features.statisticsDays ?? days
        ThemedList {
            Picker("Period", selection: $days) {
                ForEach(Self.spans, id: \.0) { Text($0.1).tag($0.0) }
            }
            .pickerStyle(.segmented)
            if let statistics, let detail {
                Section {
                    StatisticsTiles(statistics: statistics, detail: detail, span: loadedDays)
                        .listRowInsets(EdgeInsets())
                        .listRowBackground(Color.clear)
                }
                if statistics.plays == 0 {
                    ContentUnavailableView(
                        "No plays yet",
                        systemImage: "chart.bar",
                        description: Text("Play a song for half its length, or four minutes, to count a play.")
                    )
                    .listRowBackground(Color.clear)
                } else {
                    if !detail.days.isEmpty {
                        Section("Listening over time") {
                            ListeningChart(days: detail.days, span: loadedDays, selected: $selected)
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
                ProgressView("Reading your listening history…")
            }
        }
        .toolbar {
            if statistics != nil, features.statisticsLoading {
                ProgressView()
            }
        }
        .navigationTitle("Statistics")
        .refreshable { features.loadStatistics(days: days) }
        .task(id: days) {
            selected = nil
            features.loadStatistics(days: days)
        }
    }

    private func shown<Row>(_ rows: [Row], _ rank: StatisticsRank) -> [Row] {
        Array(rows.prefix(widened.contains(rank) ? Self.longList : Self.shortList))
    }

    private func more(_ count: Int, _ rank: StatisticsRank) -> some View {
        Group {
            if count > Self.shortList {
                Button(widened.contains(rank) ? "Show fewer" : "Show more") {
                    if !widened.insert(rank).inserted { widened.remove(rank) }
                }
            }
        }
    }

    @ViewBuilder
    private func songs(_ tracks: [Track]) -> some View {
        if !tracks.isEmpty {
            Section("Most listened songs") {
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
            Section("Most listened albums") {
                ForEach(shown(albums, .albums), id: \.key) { album in
                    NavigationLink(value: Destination.album(album.key)) {
                        ListenedToRow(item: album)
                    }
                }
                more(albums.count, .albums)
            }
        }
    }

    @ViewBuilder
    private func artists(_ artists: [ListenedTo]) -> some View {
        if !artists.isEmpty {
            Section("Most listened artists") {
                ForEach(shown(artists, .artists), id: \.key) { artist in
                    NavigationLink(value: Destination.artist(artist.name)) {
                        ListenedToRow(item: artist)
                    }
                }
                more(artists.count, .artists)
            }
        }
    }

    static func spoken(_ milliseconds: UInt64) -> String {
        let minutes = Int(milliseconds / 60_000)
        let hours = minutes / 60
        let rest = minutes % 60
        if hours == 0 { return counted(rest, "minute") }
        if rest == 0 { return counted(hours, "hour") }
        return "\(counted(hours, "hour")) \(counted(rest, "minute"))"
    }

    static func brief(_ milliseconds: UInt64) -> String {
        let minutes = Int(milliseconds / 60_000)
        if minutes < 60 { return "\(minutes) min" }
        let hours = minutes / 60
        if hours < 100 { return "\(hours) h \(minutes % 60) min" }
        return "\(hours) h"
    }
}

enum StatisticsRank: Hashable {
    case songs, albums, artists
}

struct StatTile: View {
    private static let corner: CGFloat = 16

    let title: String
    let value: String
    let detail: String?
    let symbol: String

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Label(title, systemImage: symbol)
                .font(.caption.weight(.medium))
                .foregroundStyle(.secondary)
                .labelStyle(.titleAndIcon)
            Text(value)
                .font(.title2.weight(.semibold))
                .minimumScaleFactor(0.6)
                .lineLimit(1)
            Text(detail ?? " ")
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .background(fill, in: RoundedRectangle(cornerRadius: Self.corner, style: .continuous))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(title)
        .accessibilityValue(detail.map { "\(value), \($0)" } ?? value)
    }

    private var fill: Color {
        AppearanceState.shared.themedBackground ? Color.themedCard : Color(.secondarySystemGroupedBackground)
    }
}

struct StatisticsTiles: View {
    let statistics: LibraryStatistics
    let detail: ListeningDetail
    let span: UInt32

    var body: some View {
        let busiest = detail.days.max { $0.listenedMs < $1.listenedMs }
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 150), spacing: 12)], spacing: 12) {
            StatTile(
                title: "Listened",
                value: StatisticsView.brief(statistics.listenedMs),
                detail: counted(Int(statistics.plays), "play"),
                symbol: "headphones"
            )
            StatTile(
                title: "Daily average",
                value: StatisticsView.brief(average),
                detail: span == 0 ? "since your first play" : "over \(span) days",
                symbol: "calendar"
            )
            StatTile(
                title: "Songs heard",
                value: "\(detail.songsHeard)",
                detail: "of \(statistics.songs) in your library",
                symbol: "music.note"
            )
            StatTile(
                title: "Busiest day",
                value: busiest.map { StatisticsView.brief($0.listenedMs) } ?? "None",
                detail: busiest.map { Date(timeIntervalSince1970: TimeInterval($0.starts)).formatted(span == 0 || span == 365 ? .dateTime.day().month(.abbreviated).year() : .dateTime.day().month(.abbreviated)) },
                symbol: "flame"
            )
            StatTile(
                title: "Your library",
                value: counted(Int(statistics.songs), "song"),
                detail: StatisticsView.brief(statistics.durationMs),
                symbol: "music.note.house"
            )
            StatTile(
                title: "Albums and artists",
                value: "\(statistics.albums) · \(statistics.artists)",
                detail: "albums · artists",
                symbol: "square.stack"
            )
        }
        .padding(.vertical, 4)
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

struct ListenedToRow: View {
    let item: ListenedTo

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(item.name)
                .lineLimit(1)
            Text(detail)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .accessibilityElement(children: .combine)
    }

    private var detail: String {
        let heard = "\(counted(Int(item.plays), "play")) · \(StatisticsView.spoken(item.listenedMs))"
        return item.by.isEmpty ? heard : "\(item.by) · \(heard)"
    }
}

struct ListeningBar: Identifiable, Equatable {
    let starts: Date
    let listenedMs: UInt64
    let plays: UInt64

    var id: Date { starts }

    static func unit(for span: UInt32) -> Calendar.Component {
        switch span {
        case 0: .month
        case 365: .weekOfYear
        default: .day
        }
    }

    static func grouped(_ days: [ListenedOnDay], span: UInt32, calendar: Calendar = .current) -> [ListeningBar] {
        let unit = unit(for: span)
        var bars: [Date: ListeningBar] = [:]
        for day in days {
            let moment = Date(timeIntervalSince1970: TimeInterval(day.starts))
            let key = calendar.dateInterval(of: unit, for: moment)?.start ?? moment
            let held = bars[key]
            bars[key] = ListeningBar(
                starts: key,
                listenedMs: (held?.listenedMs ?? 0) + day.listenedMs,
                plays: (held?.plays ?? 0) + day.plays
            )
        }
        return bars.values.sorted { $0.starts < $1.starts }
    }

    static func edgeFormat(for span: UInt32) -> Date.FormatStyle {
        switch span {
        case 0: .dateTime.month(.abbreviated).year()
        case 365: .dateTime.day().month(.abbreviated).year()
        default: .dateTime.day().month(.abbreviated)
        }
    }

    func caption(span: UInt32) -> String {
        switch span {
        case 0: starts.formatted(.dateTime.month(.wide).year())
        case 365: "Week of " + starts.formatted(.dateTime.day().month(.abbreviated))
        default: starts.formatted(.dateTime.weekday(.wide).day().month(.abbreviated))
        }
    }
}

struct ListeningChart: View {
    let days: [ListenedOnDay]
    let span: UInt32
    @Binding var selected: Date?

    var body: some View {
        let bars = ListeningBar.grouped(days, span: span)
        ListeningBars(bars: bars, span: span, tallest: max(bars.map(\.listenedMs).max() ?? 1, 1), selected: $selected)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Listening over time")
            .accessibilityValue(summary(bars))
    }

    private func summary(_ bars: [ListeningBar]) -> String {
        guard let busiest = bars.max(by: { $0.listenedMs < $1.listenedMs }) else { return "Nothing heard" }
        return "\(bars.count) periods. The most was \(StatisticsView.spoken(busiest.listenedMs)), \(busiest.caption(span: span))"
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
                Text("\(chosen.caption(span: span)) · \(counted(Int(chosen.plays), "play"))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        } else {
            VStack(alignment: .leading, spacing: 2) {
                Text(StatisticsView.spoken(bars.reduce(0) { $0 + $1.listenedMs })).font(.headline)
                Text("Touch the chart to see a \(unitName)")
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

    private var unitName: String {
        switch span {
        case 0: "month"
        case 365: "week"
        default: "day"
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
                    Text("\(counted(Int(share.songs), "song")) · \(fraction.formatted(.percent.precision(.fractionLength(0))))")
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
            .accessibilityValue("\(counted(Int(share.songs), "song")), \(fraction.formatted(.percent.precision(.fractionLength(0))))")
        }
    }
}

struct LibraryBreakdownSections: View {
    let breakdown: LibraryBreakdown
    let songs: UInt64

    var body: some View {
        if !breakdown.formats.isEmpty {
            Section("Formats") { ShareBars(shares: breakdown.formats, songs: songs) }
        }
        if !breakdown.rates.isEmpty {
            Section("Sample rates") { ShareBars(shares: breakdown.rates, songs: songs) }
        }
        if !breakdown.genres.isEmpty {
            Section("Genres") { ShareBars(shares: breakdown.genres, songs: songs) }
        }
        if !breakdown.decades.isEmpty {
            Section("Decades") { ShareBars(shares: breakdown.decades, songs: songs) }
        }
    }
}
