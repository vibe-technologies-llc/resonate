import ResonateWidgetLinks
import SwiftUI
import WidgetKit

@main
struct PlaybackWidgetBundle: WidgetBundle {
    var body: some Widget {
        PlaybackWidget()
    }
}

struct PlaybackWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "playback-shortcuts", provider: ShortcutsTimeline()) { _ in
            PlaybackShortcuts()
                .containerBackground(.fill.tertiary, for: .widget)
                .widgetURL(WidgetLink.nowPlaying.url)
        }
        .configurationDisplayName(Text("widget.title"))
        .description(Text("widget.description"))
        .supportedFamilies([.systemSmall, .systemMedium])
    }
}

private struct ShortcutsTimeline: TimelineProvider {
    struct Entry: TimelineEntry {
        let date = Date()
    }

    func placeholder(in context: Context) -> Entry { Entry() }

    func getSnapshot(in context: Context, completion: @escaping (Entry) -> Void) {
        completion(Entry())
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<Entry>) -> Void) {
        completion(Timeline(entries: [Entry()], policy: .never))
    }
}

private struct PlaybackShortcuts: View {
    @Environment(\.widgetFamily) private var family

    var body: some View {
        if family == .systemSmall {
            VStack(alignment: .leading, spacing: 8) {
                Image(systemName: "play.circle.fill")
                    .font(.largeTitle)
                    .widgetAccentable()
                    .accessibilityHidden(true)
                Spacer(minLength: 0)
                Text("widget.now-playing")
                    .font(.headline)
                    .lineLimit(2)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        } else {
            HStack(spacing: 8) {
                shortcut(.nowPlaying, title: Text("widget.now-playing"), symbol: "play.circle.fill")
                shortcut(.shuffle, title: Text("widget.shuffle"), symbol: "shuffle")
                shortcut(.search, title: Text("search.title"), symbol: "magnifyingglass")
            }
        }
    }

    private func shortcut(_ route: WidgetLink, title: Text, symbol: String) -> some View {
        Link(destination: route.url) {
            VStack(spacing: 6) {
                Image(systemName: symbol)
                    .font(.title2)
                    .widgetAccentable()
                    .accessibilityHidden(true)
                title
                    .font(.caption.weight(.semibold))
                    .multilineTextAlignment(.center)
                    .lineLimit(2)
                    .minimumScaleFactor(0.8)
            }
            .padding(.horizontal, 4)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(.fill.secondary, in: RoundedRectangle(cornerRadius: 16))
        }
    }
}
