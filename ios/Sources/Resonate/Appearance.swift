import Observation
import ResonateLogic
import ResonateNative
import SwiftUI
import UIKit

@MainActor
@Observable
final class AppearanceState {
    static let shared = AppearanceState()

    private static let accentKey = "accent"
    private static let backgroundKey = "themedBackground"
    private static let movingKey = "movingBackground"
    private static let strengthKey = "backgroundStrength"
    private static let intensityKey = "nowPlayingIntensity"
    static let defaultStrength = 0.4
    static let defaultIntensity = Double(BackdropTone.neutralIntensity)

    @ObservationIgnored private let settings = UserDefaults(suiteName: "appearance") ?? .standard

    private(set) var accent: ResonateAccent? {
        didSet {
            tint = Self.tint(of: accent)
            ink = Self.ink(of: accent)
            resolvedTints = [:]
        }
    }
    private(set) var tint = Color.accentColor
    private(set) var ink = Color.white
    private(set) var iconRefused: String?
    @ObservationIgnored private var iconChange: Task<Void, Never>?
    @ObservationIgnored private var resolvedTints: [TintLook: Color.Resolved] = [:]

    var themedBackground: Bool {
        didSet { settings.set(themedBackground, forKey: Self.backgroundKey) }
    }

    var movingBackground: Bool {
        didSet { settings.set(movingBackground, forKey: Self.movingKey) }
    }

    var backgroundStrength: Double {
        didSet { keepLevelsSoon() }
    }

    var nowPlayingIntensity: Double {
        didSet { keepLevelsSoon() }
    }

    @ObservationIgnored private var keepingLevels: Task<Void, Never>?
    private static let levelsKeptAfter = Duration.milliseconds(400)

    private init() {
        let kept = settings.string(forKey: Self.accentKey)
        let chosen = ResonateAccent.every.first { $0.name == kept }
        accent = chosen
        tint = Self.tint(of: chosen)
        ink = Self.ink(of: chosen)
        themedBackground = settings.object(forKey: Self.backgroundKey) as? Bool ?? true
        movingBackground = settings.object(forKey: Self.movingKey) as? Bool ?? true
        backgroundStrength = settings.object(forKey: Self.strengthKey) as? Double ?? Self.defaultStrength
        nowPlayingIntensity = settings.object(forKey: Self.intensityKey) as? Double ?? Self.defaultIntensity
        iconChange = Task { [weak self] in
            guard let self else { return }
            await showIcon(of: accent)
        }
    }

    private func keepLevelsSoon() {
        keepingLevels?.cancel()
        keepingLevels = Task { [weak self] in
            try? await Task.sleep(for: Self.levelsKeptAfter)
            guard !Task.isCancelled, let self else { return }
            settings.set(backgroundStrength, forKey: Self.strengthKey)
            settings.set(nowPlayingIntensity, forKey: Self.intensityKey)
        }
    }

    private static func tint(of accent: ResonateAccent?) -> Color {
        accent.map(color) ?? Color.accentColor
    }

    private static func ink(of accent: ResonateAccent?) -> Color {
        accent.map(ink(on:)) ?? .white
    }

    static func ink(on accent: ResonateAccent) -> Color {
        AccentColours.ink(on: accent.hex)
    }

    func wear(_ chosen: ResonateAccent?) {
        accent = chosen
        if let chosen {
            settings.set(chosen.name, forKey: Self.accentKey)
        } else {
            settings.removeObject(forKey: Self.accentKey)
        }
        let previous = iconChange
        iconChange = Task { [weak self] in
            await previous?.value
            guard let self else { return }
            await showIcon(of: accent)
        }
    }

    func resetIcon() async {
        await iconChange?.value
        await showIcon(of: nil)
    }

    private func showIcon(of chosen: ResonateAccent?) async {
        let application = UIApplication.shared
        guard application.supportsAlternateIcons else { return }
        let wanted = chosen?.alternateIcon
        guard application.alternateIconName != wanted else { return }
        do {
            try await application.setAlternateIconName(wanted)
            iconRefused = nil
        } catch {
            iconRefused = error.localizedDescription
        }
    }

    static func color(of accent: ResonateAccent) -> Color {
        AccentColours.worn(accent.hex)
    }

    func resolvedTint(_ scheme: ColorScheme, contrast: ColorSchemeContrast = .standard) -> Color.Resolved {
        let look = TintLook(scheme: scheme, contrast: contrast)
        if let held = resolvedTints[look] { return held }

        var values = EnvironmentValues()
        values.colorScheme = scheme
        values._colorSchemeContrast = contrast
        let resolved = tint.resolve(in: values)
        resolvedTints[look] = resolved
        return resolved
    }
}

private struct TintLook: Hashable {
    let scheme: ColorScheme
    let contrast: ColorSchemeContrast
}

enum AccentColours {
    private static let cardWhiteInDark: CGFloat = 0.11
    private static let cardOpacityInDark: CGFloat = 0.62
    private static let cardOpacityInLight: CGFloat = 0.66

    static let card = Color(UIColor { traits in
        traits.userInterfaceStyle == .dark
            ? UIColor(white: cardWhiteInDark, alpha: cardOpacityInDark)
            : UIColor(white: 1, alpha: cardOpacityInLight)
    })

    static func worn(_ hex: UInt32) -> Color {
        Color(UIColor { traits in
            let worn = AccentMath.worn(hex, dark: traits.userInterfaceStyle == .dark, increased: traits.accessibilityContrast == .high)
            return UIColor(red: worn.red, green: worn.green, blue: worn.blue, alpha: 1)
        })
    }

    static func statement(_ statement: AccentMath.Statement) -> Color {
        Color(UIColor { traits in
            let ink = statement.ink(dark: traits.userInterfaceStyle == .dark)
            return UIColor(red: ink.red, green: ink.green, blue: ink.blue, alpha: 1)
        })
    }

    static func ink(on hex: UInt32) -> Color {
        Color(UIColor { traits in
            switch AccentMath.ink(on: hex, dark: traits.userInterfaceStyle == .dark, increased: traits.accessibilityContrast == .high) {
            case .white: .white
            case .dark(let white): UIColor(white: white, alpha: 1)
            }
        })
    }
}

extension Color {
    static let failure = AccentColours.statement(.failure)
    static let warning = AccentColours.statement(.warning)
    static let confirmation = AccentColours.statement(.confirmation)

    @MainActor static var worn: Color { AppearanceState.shared.tint }
    @MainActor static var wornInk: Color { AppearanceState.shared.ink }
    static let themedCard = AccentColours.card
}

extension View {
    func touchTarget() -> some View {
        frame(minWidth: 44, minHeight: 44).contentShape(.rect)
    }

    func titleLines(_ lines: Int = 1) -> some View {
        modifier(TitleLines(lines: lines))
    }

    func buttonShape() -> some View {
        modifier(ButtonShape())
    }

    func announced(_ text: String?) -> some View {
        modifier(Announced(text: text))
    }
}

private struct Announced: ViewModifier {
    let text: String?

    func body(content: Content) -> some View {
        content
            .onAppear { speak(text) }
            .onChange(of: text) { _, now in speak(now) }
    }

    private func speak(_ text: String?) {
        guard let text else { return }
        AccessibilityNotification.Announcement(text).post()
    }
}

struct HeadedSection<Content: View>: View {
    let title: String
    let content: Content

    init(_ title: String, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    var body: some View {
        Section {
            content
        } header: {
            Text(title).accessibilityAddTraits(.isHeader)
        }
    }
}

private struct TitleLines: ViewModifier {
    @Environment(\.dynamicTypeSize) private var size
    let lines: Int

    func body(content: Content) -> some View {
        content.lineLimit(size.isAccessibilitySize ? nil : lines)
    }
}

private struct ButtonShape: ViewModifier {
    @Environment(\.accessibilityShowButtonShapes) private var shown

    func body(content: Content) -> some View {
        if shown {
            content.background(.quaternary, in: .capsule)
        } else {
            content
        }
    }
}

struct AppearanceView: View {
    @ScaledMetric(relativeTo: .body) private var swatch: CGFloat = 44
    @ScaledMetric(relativeTo: .body) private var iconEdge: CGFloat = 72

    var body: some View {
        let appearance = AppearanceState.shared
        ThemedForm {
            Section {
                HStack(spacing: 16) {
                    Image(uiImage: icon(of: appearance.accent))
                        .resizable()
                        .frame(width: iconEdge, height: iconEdge)
                        .clipShape(.rect(cornerRadius: iconEdge * 0.225, style: .continuous))
                        .accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 4) {
                        Text(appearance.accent.map { $0.name.capitalized } ?? "Resonate").font(.headline)
                        Text("Tints Resonate and its icon on the Home Screen.")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                }
                .padding(.vertical, 4)
                LazyVGrid(columns: [GridItem(.adaptive(minimum: swatch + 12), spacing: 12)], spacing: 16) {
                    AccentSwatch(name: "Resonate", fill: AnyShapeStyle(Self.packaged), ink: .white, chosen: appearance.accent == nil, edge: swatch) {
                        appearance.wear(nil)
                    }
                    ForEach(ResonateAccent.every) { accent in
                        AccentSwatch(
                            name: accent.name.capitalized,
                            fill: AnyShapeStyle(AppearanceState.color(of: accent)),
                            ink: AppearanceState.ink(on: accent),
                            chosen: appearance.accent == accent,
                            edge: swatch
                        ) {
                            appearance.wear(accent)
                        }
                    }
                }
                .padding(.vertical, 8)
            } header: {
                Text("Accent colour")
            } footer: {
                if let refused = appearance.iconRefused {
                    Text("The icon could not be changed: \(refused)")
                } else {
                    Text("Resonate keeps its own colours and icon. Any other accent recolours the icon as the desktop does.")
                }
            }
        }
        .navigationTitle("Accent colour")
    }

    private static let packaged = LinearGradient(
        colors: [Color(red: 0x6E / 255, green: 0xA8 / 255, blue: 0xFE / 255), Color(red: 0x63 / 255, green: 0xD2 / 255, blue: 0x9B / 255)],
        startPoint: .topLeading,
        endPoint: .bottomTrailing
    )

    private func icon(of accent: ResonateAccent?) -> UIImage {
        UIImage(named: accent?.iconName ?? "Icon") ?? UIImage()
    }
}

private struct CalmedAnimation<Value: Equatable>: ViewModifier {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let animation: Animation?
    let calm: Animation?
    let value: Value

    func body(content: Content) -> some View {
        content.animation(reduceMotion ? calm : animation, value: value)
    }
}

extension Animation {
    static func moving(_ curve: MotionCurve, over seconds: Double) -> Animation {
        .timingCurve(curve.x1, curve.y1, curve.x2, curve.y2, duration: seconds)
    }
}

enum Handover {
    static let animation = Animation.moving(.smooth, over: MotionSpan.handsOver)
}

enum Arrival {
    static let animation = Animation.moving(.settling, over: MotionSpan.arrivesOver)
}

enum Flip {
    static let animation = Animation.moving(.settling, over: MotionSpan.flipsOver)
}

extension MotionRendering {
    func animation(_ animated: Animation) -> Animation? {
        switch self {
        case .animated: animated
        case .fades: Arrival.animation
        case .instant: nil
        }
    }
}

private struct HandedOver<Value: Equatable>: ViewModifier {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.scenePhase) private var phase
    let value: Value

    func body(content: Content) -> some View {
        let rendering = MotionRendering.of(.handover, reduceMotion: reduceMotion, sceneActive: phase == .active)
        content.animation(rendering.animation(Handover.animation), value: value)
    }
}

extension View {
    func handover<Value: Equatable>(value: Value) -> some View {
        modifier(HandedOver(value: value))
    }

    func motion<Value: Equatable>(_ animation: Animation?, calm: Animation? = nil, value: Value) -> some View {
        modifier(CalmedAnimation(animation: animation, calm: calm, value: value))
    }
}

struct BackgroundView: View {
    var body: some View {
        @Bindable var appearance = AppearanceState.shared
        ThemedForm {
            Section {
                Toggle("Resonate background", isOn: $appearance.themedBackground)
                if appearance.themedBackground {
                    Toggle("Flowing", isOn: $appearance.movingBackground)
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Strength")
                        HStack(spacing: 12) {
                            Text("Subtle").font(.caption).foregroundStyle(.secondary)
                            Slider(value: $appearance.backgroundStrength, in: 0...1)
                                .accessibilityLabel("Strength")
                                .accessibilityValue(Text(appearance.backgroundStrength, format: .percent.precision(.fractionLength(0))))
                            Text("Vivid").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
            } footer: {
                Text("A slow field in the accent colour behind every page, drawn as Now Playing draws the cover's. Reduce Motion keeps it still, and Low Power Mode slows it.")
            }
            Section {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Intensity")
                    HStack(spacing: 12) {
                        Text("Darker").font(.caption).foregroundStyle(.secondary)
                        Slider(value: $appearance.nowPlayingIntensity, in: 0...1)
                            .accessibilityLabel("Now Playing intensity")
                            .accessibilityValue(Text(appearance.nowPlayingIntensity, format: .percent.precision(.fractionLength(0))))
                        Text("Vibrant").font(.caption).foregroundStyle(.secondary)
                    }
                }
                if appearance.nowPlayingIntensity != AppearanceState.defaultIntensity {
                    Button("Reset to default") { appearance.nowPlayingIntensity = AppearanceState.defaultIntensity }
                }
            } header: {
                Text("Now Playing")
            } footer: {
                Text("How dark or vivid the cover's colours are behind Now Playing, its lyrics and the visualizer.")
            }
        }
        .navigationTitle("Background")
    }
}

struct AccentSwatch: View {
    let name: String
    let fill: AnyShapeStyle
    let ink: Color
    let chosen: Bool
    let edge: CGFloat
    let choose: () -> Void

    var body: some View {
        Button(action: choose) {
            VStack(spacing: 6) {
                Circle()
                    .fill(fill)
                    .frame(width: edge, height: edge)
                    .overlay {
                        if chosen {
                            Image(systemName: "checkmark")
                                .font(.body.weight(.bold))
                                .foregroundStyle(ink)
                                .accessibilityHidden(true)
                        }
                    }
                    .padding(3)
                    .overlay(Circle().strokeBorder(chosen ? Color.primary : .clear, lineWidth: 2))
                Text(name).font(.caption).foregroundStyle(chosen ? .primary : .secondary).titleLines()
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(name)
        .accessibilityAddTraits(chosen ? .isSelected : [])
    }
}
