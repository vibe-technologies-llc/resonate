import ResonateLogic
import ResonateNative
import SwiftUI
import UIKit

struct WrappedDeck: View {
    private static let previewScale: CGFloat = 1
    private static let previewAfter = Duration.milliseconds(450)

    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var shown: WrappedYear
    @State private var asked: Int64?
    @State private var at = 0
    @State private var copies = 0
    @State private var preview: Image?
    @State private var failure: String?

    init(year: WrappedYear) {
        _shown = State(initialValue: year)
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                TabView(selection: $at) {
                    ForEach(Array(shown.slides.enumerated()), id: \.offset) { place, slide in
                        WrappedSlideView(slide: slide).tag(place)
                    }
                }
                .tabViewStyle(.page(indexDisplayMode: .never))
                .id(shown.year)
                if let failure {
                    Text(failure)
                        .font(.footnote)
                        .foregroundStyle(Color.failure)
                        .announced(failure)
                        .padding(.horizontal)
                }
                turns
            }
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { toolbar }
            .modifier(ThemedPage())
            .sensoryFeedback(.success, trigger: copies)
            .task(id: shown.year) { await drawPreview() }
            .task(id: asked) { await read() }
        }
        .tint(AppearanceState.shared.tint)
    }

    private var turns: some View {
        HStack {
            Button("wrapped.back", systemImage: "chevron.left") { turn(by: -1) }
                .disabled(at == 0)
            Spacer()
            Text(WrappedDeckPlace.said(at: at, of: shown.slides.count))
                .font(.footnote.monospacedDigit())
                .foregroundStyle(.secondary)
            Spacer()
            Button("wrapped.next", systemImage: "chevron.right") { turn(by: 1) }
                .disabled(at >= shown.slides.count - 1)
        }
        .buttonStyle(.glass)
        .padding()
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .cancellationAction) { Button(role: .close) { dismiss() } }
        if shown.years.count > 1 {
            ToolbarItem(placement: .principal) {
                Menu(String(shown.year)) {
                    ForEach(shown.years, id: \.self) { year in
                        Button(String(year)) { asked = year }
                    }
                }
                .accessibilityLabel(said("wrapped.year.label", String(shown.year)))
            }
        }
        ToolbarItem(placement: .topBarTrailing) {
            Button(said(copies > 0 ? "wrapped.copied" : "wrapped.copy-summary"), systemImage: "doc.on.doc") {
                UIPasteboard.general.string = shown.summary
                copies += 1
            }
            .announced(copies > 0 ? said("wrapped.copied") : nil)
        }
        if let words = WrappedCardWords.of(shown), let preview {
            ToolbarItem(placement: .topBarTrailing) {
                ShareLink(
                    item: WrappedCardShare(words: words, accent: AppearanceState.shared.tint),
                    preview: SharePreview(said("wrapped.share.title", String(shown.year)), image: preview)
                ) {
                    Label("wrapped.share-card", systemImage: "square.and.arrow.up")
                }
            }
        }
    }

    private func turn(by step: Int) {
        let next = WrappedDeckPlace.turned(from: at, by: step, of: shown.slides.count)
        withAnimation(reduceMotion ? nil : Arrival.animation) { at = next }
    }

    private func read() async {
        guard let asked, asked != shown.year else { return }
        do {
            let found = try await library.features.wrapped(of: asked)
            guard !Task.isCancelled else { return }
            failure = nil
            if let found {
                at = 0
                copies = 0
                shown = found
            }
        } catch {
            failure = explained(error)
        }
    }

    private func drawPreview() async {
        guard let words = WrappedCardWords.of(shown) else {
            preview = nil
            return
        }
        if preview == nil {
            try? await Task.sleep(for: Self.previewAfter)
            guard !Task.isCancelled else { return }
        }
        let renderer = ImageRenderer(content: WrappedCard(words: words, accent: AppearanceState.shared.tint))
        renderer.scale = Self.previewScale
        preview = renderer.uiImage.map { Image(uiImage: $0) }
    }
}

struct WrappedSlideView: View {
    private static let widest: CGFloat = 560

    let slide: WrappedSlide

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                VStack(alignment: .leading, spacing: 8) {
                    Text(slide.eyebrow.uppercased())
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                    Text(slide.headline)
                        .font(.largeTitle.bold())
                    if let detail = slide.detail {
                        Text(detail).foregroundStyle(.secondary)
                    }
                }
                .accessibilityElement(children: .combine)
                .accessibilityAddTraits(.isHeader)
                if !slide.rows.isEmpty {
                    WrappedRows(rows: slide.rows)
                }
                if !slide.bars.isEmpty {
                    WrappedBars(bars: slide.bars)
                }
            }
            .frame(maxWidth: Self.widest, alignment: .leading)
            .padding(24)
            .frame(maxWidth: .infinity)
        }
        .scrollBounceBehavior(.basedOnSize)
    }
}

struct WrappedRows: View {
    let rows: [WrappedRow]

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            ForEach(Array(rows.enumerated()), id: \.offset) { place, row in
                HStack(alignment: .firstTextBaseline, spacing: 12) {
                    Text("\(place + 1)")
                        .font(.subheadline.monospacedDigit())
                        .foregroundStyle(.secondary)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(row.name).font(.body.weight(.medium)).titleLines()
                        if let by = row.by {
                            Text(by).font(.subheadline).foregroundStyle(.secondary).titleLines()
                        }
                    }
                    Spacer(minLength: 8)
                    Text(row.figure)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
                .accessibilityElement(children: .combine)
            }
        }
    }
}

struct WrappedBars: View {
    private static let permille = 1000.0
    private static let shortest: CGFloat = 2
    private static let gap: CGFloat = 2

    @Environment(\.dynamicTypeSize) private var typeSize
    @ScaledMetric(relativeTo: .body) private var tallest: CGFloat = 160
    let bars: [WrappedBar]

    var body: some View {
        let every = WrappedDeckPlace.labelledEvery(bars: bars.count, accessibilityText: typeSize.isAccessibilitySize)
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .bottom, spacing: Self.gap) {
                ForEach(Array(bars.enumerated()), id: \.offset) { _, bar in
                    RoundedRectangle(cornerRadius: Self.gap)
                        .fill(bar.peak ? Color.accentColor : Color.secondary.opacity(0.4))
                        .frame(height: max(Self.shortest, tallest * Double(bar.sharePermille) / Self.permille))
                        .frame(maxWidth: .infinity)
                        .accessibilityElement()
                        .accessibilityLabel(bar.said)
                }
            }
            .frame(height: tallest, alignment: .bottom)
            HStack(spacing: Self.gap) {
                ForEach(Array(bars.enumerated()), id: \.offset) { place, bar in
                    Text(place % every == 0 ? bar.label : "")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .fixedSize()
                        .frame(maxWidth: .infinity)
                }
            }
            .accessibilityHidden(true)
            if let peak = bars.first(where: \.peak) {
                Text(peak.said)
                    .font(.footnote.weight(.semibold))
                    .accessibilityHidden(true)
            }
        }
    }
}

enum WrappedCardFailure: Swift.Error {
    case notDrawn
}

struct WrappedCardShare: Transferable, Sendable {
    private static let scale: CGFloat = 3

    let words: WrappedCardWords
    let accent: Color

    static var transferRepresentation: some TransferRepresentation {
        DataRepresentation(exportedContentType: .png) { share in try await share.png() }
    }

    @MainActor
    private func png() throws -> Data {
        let renderer = ImageRenderer(content: WrappedCard(words: words, accent: accent))
        renderer.scale = Self.scale
        guard let drawn = renderer.uiImage?.pngData() else { throw WrappedCardFailure.notDrawn }
        return drawn
    }
}

struct WrappedCard: View {
    private static let width: CGFloat = 360
    private static let height: CGFloat = 450
    private static let margin: CGFloat = 30
    private static let ground = Color(red: 0.086, green: 0.094, blue: 0.114)

    let words: WrappedCardWords
    let accent: Color

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            VStack(alignment: .leading, spacing: 6) {
                Text(words.eyebrow.uppercased())
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(accent)
                Text(words.headline)
                    .font(.title.bold())
                    .lineLimit(3)
                    .minimumScaleFactor(0.7)
                if let detail = words.detail {
                    Text(detail)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .lineLimit(3)
                }
            }
            ForEach(Array(words.rows.enumerated()), id: \.offset) { _, row in
                VStack(alignment: .leading, spacing: 1) {
                    Text(row.label)
                        .font(.caption2.weight(.medium))
                        .foregroundStyle(.secondary)
                    Text(row.name)
                        .font(.headline)
                        .lineLimit(1)
                    if let by = row.by {
                        Text(by)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                }
            }
            Spacer(minLength: 0)
        }
        .padding(Self.margin)
        .frame(width: Self.width, height: Self.height, alignment: .topLeading)
        .background(Self.ground)
        .foregroundStyle(.white)
        .environment(\.colorScheme, .dark)
        .dynamicTypeSize(.large)
    }
}
