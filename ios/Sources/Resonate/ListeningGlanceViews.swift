import Charts
import ResonateLogic
import ResonateNative
import SwiftUI

struct HomeListeningGlance: View {
    let feed: HomeFeed

    var body: some View {
        Section {
            ListeningGlanceLink(glance: feed.glance)
        } header: {
            SeeAllPageHeader(title: said("glance.title"), more: true, page: .statistics)
        }
    }
}

private struct ListeningGlanceLink: View {
    let glance: ListeningGlance?

    var body: some View {
        ScrollView(.horizontal) {
            NavigationLink(value: Destination.statistics) {
                ListeningGlanceCard(read: glance)
                    .containerRelativeFrame(.horizontal)
            }
            .buttonStyle(.plain)
        }
        .scrollDisabled(true)
        .scrollIndicators(.hidden)
    }
}

private struct ListeningGlanceCard: View {
    private static var readingLabel: String { said("statistics.reading") }
    private static var unread: ListeningGlance { ListeningGlance.unread() }

    @Environment(\.horizontalSizeClass) private var horizontal
    @Environment(\.dynamicTypeSize) private var typeSize
    let read: ListeningGlance?

    var body: some View {
        let glance = read ?? Self.unread
        arranged(glance)
            .padding(.vertical, 6)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(.rect)
            .redacted(reason: read == nil ? .placeholder : [])
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(read == nil ? Self.readingLabel : glance.spoken)
            .accessibilityHint("glance.open.hint")
    }

    @ViewBuilder
    private func arranged(_ glance: ListeningGlance) -> some View {
        let drawsChart = glance.standing != .restingThisWeek
        switch ListeningGlanceArrangement.of(horizontal: SizeClass(horizontal), accessibilityText: typeSize.isAccessibilitySize) {
        case .stacked:
            VStack(alignment: .leading, spacing: 14) {
                ListeningGlanceHeadline(glance: glance)
                if drawsChart { ListeningWeekChart(glance: glance) }
                ListeningFacts(facts: glance.facts)
            }
        case .beside:
            HStack(alignment: .top, spacing: 24) {
                VStack(alignment: .leading, spacing: 14) {
                    ListeningGlanceHeadline(glance: glance)
                    ListeningFacts(facts: glance.facts)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                if drawsChart {
                    ListeningWeekChart(glance: glance)
                        .frame(maxWidth: .infinity)
                }
            }
        }
    }
}

private struct ListeningGlanceHeadline: View {
    let glance: ListeningGlance

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(glance.eyebrow)
                .font(.subheadline.weight(.medium))
                .foregroundStyle(.secondary)
            Text(glance.headline)
                .font(glance.standing == .nothingHeard ? .title2.bold() : .largeTitle.bold())
                .monospacedDigit()
            Text(glance.detail)
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .fixedSize(horizontal: false, vertical: true)
    }
}

private struct ListeningWeekChart: View {
    private static let millisecondsAMinute = 60_000.0
    private static let stubOfTheTallest = 0.02
    private static let lowestStubMinutes = 1.0
    private static let barWidthOfADay = 0.62
    private static let barCorner: CGFloat = 4
    private static let quietBarOpacity = 0.4
    private static let quietBarOpacityUnderContrast = 0.7
    private static let restingStubOpacity = 0.25
    private static let dashes: [CGFloat] = [3, 3]
    private static let averageLabel = "avg"

    @Environment(\.colorSchemeContrast) private var contrast
    @ScaledMetric(relativeTo: .caption) private var height: CGFloat = 96
    let glance: ListeningGlance

    var body: some View {
        let tallest = Self.minutes(glance.tallestMs)
        let stub = max(tallest * Self.stubOfTheTallest, Self.lowestStubMinutes * Self.stubOfTheTallest)
        let average = Self.minutes(glance.averageMs)
        Chart {
            ForEach(glance.week) { day in
                BarMark(
                    x: .value(said("glance.chart.day"), day.starts, unit: .day),
                    y: .value(said("glance.chart.minutes"), max(Self.minutes(day.listenedMs), stub)),
                    width: .ratio(Self.barWidthOfADay)
                )
                .cornerRadius(Self.barCorner, style: .continuous)
                .foregroundStyle(fill(of: day))
            }
            if average > 0 {
                RuleMark(y: .value(said("glance.chart.average"), average))
                    .lineStyle(StrokeStyle(lineWidth: 1, dash: Self.dashes))
                    .foregroundStyle(.secondary)
            }
        }
        .chartYScale(domain: 0...max(tallest, Self.lowestStubMinutes))
        .chartYAxis {
            AxisMarks(position: .trailing, values: average > 0 ? [average] : []) { _ in
                AxisValueLabel {
                    Text(Self.averageLabel)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .chartXAxis {
            AxisMarks(values: .stride(by: .day)) { value in
                AxisValueLabel(centered: true) {
                    if let starts = value.as(Date.self) {
                        weekdayLabel(starts)
                    }
                }
            }
        }
        .chartLegend(.hidden)
        .frame(height: height)
        .dynamicTypeSize(...DynamicTypeSize.accessibility1)
    }

    private func weekdayLabel(_ starts: Date) -> some View {
        let isToday = glance.week.last.map { Calendar.current.isDate(starts, inSameDayAs: $0.starts) } ?? false
        return Text(starts, format: .dateTime.weekday(.narrow))
            .font(.caption2.weight(isToday ? .bold : .regular))
            .foregroundStyle(isToday ? Color.primary : Color.secondary)
    }

    private func fill(of day: ListeningGlance.Day) -> Color {
        if day.listenedMs == 0 { return Color.secondary.opacity(Self.restingStubOpacity) }
        if day.isToday { return Color.worn }
        return Color.worn.opacity(contrast == .increased ? Self.quietBarOpacityUnderContrast : Self.quietBarOpacity)
    }

    private static func minutes(_ milliseconds: UInt64) -> Double {
        Double(milliseconds) / millisecondsAMinute
    }
}

private struct ListeningFacts: View {
    @ScaledMetric(relativeTo: .subheadline) private var narrowestColumn: CGFloat = 130

    let facts: [ListeningGlance.Fact]

    var body: some View {
        if !facts.isEmpty {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: narrowestColumn), spacing: 12, alignment: .topLeading)], alignment: .leading, spacing: 12) {
                ForEach(facts) { fact in
                    ListeningFactView(fact: fact)
                }
            }
        }
    }
}

private struct ListeningFactView: View {
    let fact: ListeningGlance.Fact

    var body: some View {
        VStack(alignment: .leading, spacing: 1) {
            Text(fact.value)
                .font(.subheadline.weight(.semibold))
            Text(fact.caption)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .fixedSize(horizontal: false, vertical: true)
    }
}
