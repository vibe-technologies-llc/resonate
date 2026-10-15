import Observation
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
    static let defaultStrength = 0.4
    private static let deepenedInLight: CGFloat = 0.28
    private static let darkInkWhite: CGFloat = 0.08
    private static let darkInkLuminance: CGFloat = 0.0067

    @ObservationIgnored private let settings = UserDefaults(suiteName: "appearance") ?? .standard

    private(set) var accent: ResonateAccent?
    private(set) var iconRefused: String?

    var themedBackground: Bool {
        didSet { settings.set(themedBackground, forKey: Self.backgroundKey) }
    }

    var movingBackground: Bool {
        didSet { settings.set(movingBackground, forKey: Self.movingKey) }
    }

    var backgroundStrength: Double {
        didSet { settings.set(backgroundStrength, forKey: Self.strengthKey) }
    }

    private init() {
        let kept = settings.string(forKey: Self.accentKey)
        accent = ResonateAccent.every.first { $0.name == kept }
        themedBackground = settings.object(forKey: Self.backgroundKey) as? Bool ?? true
        movingBackground = settings.object(forKey: Self.movingKey) as? Bool ?? true
        backgroundStrength = settings.object(forKey: Self.strengthKey) as? Double ?? Self.defaultStrength
    }

    var tint: Color {
        accent.map(Self.color) ?? Color.accentColor
    }

    var ink: Color {
        accent.map(Self.ink(on:)) ?? .white
    }

    static func ink(on accent: ResonateAccent) -> Color {
        let worn = rgb(accent.hex)
        return Color(UIColor { traits in
            let deepened = traits.userInterfaceStyle == .dark ? 0 : Self.deepenedInLight
            let luminance = luminance(
                red: worn.red * (1 - deepened),
                green: worn.green * (1 - deepened),
                blue: worn.blue * (1 - deepened)
            )
            let lightContrast = (1 + 0.05) / (luminance + 0.05)
            let darkContrast = (luminance + 0.05) / (darkInkLuminance + 0.05)
            return lightContrast >= darkContrast ? .white : UIColor(white: darkInkWhite, alpha: 1)
        })
    }

    nonisolated private static func luminance(red: CGFloat, green: CGFloat, blue: CGFloat) -> CGFloat {
        func linear(_ channel: CGFloat) -> CGFloat {
            channel <= 0.04045 ? channel / 12.92 : pow((channel + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)
    }

    func wear(_ chosen: ResonateAccent?) {
        accent = chosen
        if let chosen {
            settings.set(chosen.name, forKey: Self.accentKey)
        } else {
            settings.removeObject(forKey: Self.accentKey)
        }
        Task { await showIcon(of: chosen) }
    }

    func resetIcon() async {
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
        let worn = rgb(accent.hex)
        return Color(UIColor { traits in
            let deepened = traits.userInterfaceStyle == .dark ? 0 : deepenedInLight
            return UIColor(
                red: worn.red * (1 - deepened),
                green: worn.green * (1 - deepened),
                blue: worn.blue * (1 - deepened),
                alpha: 1
            )
        })
    }

    nonisolated private static func rgb(_ hex: UInt32) -> (red: CGFloat, green: CGFloat, blue: CGFloat) {
        let channel = { (shift: UInt32) in CGFloat((hex >> shift) & 0xFF) / 255 }
        return (channel(16), channel(8), channel(0))
    }
}

extension Color {
    @MainActor static var worn: Color { AppearanceState.shared.tint }
    @MainActor static var wornInk: Color { AppearanceState.shared.ink }
}

extension View {
    func touchTarget() -> some View {
        frame(minWidth: 44, minHeight: 44).contentShape(.rect)
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

extension View {
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
                            Text("Vivid").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
            } footer: {
                Text("A slow field in the accent colour behind every page, drawn as Now Playing draws the cover's. Reduce Motion keeps it still, and Low Power Mode slows it.")
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
                        }
                    }
                    .padding(3)
                    .overlay(Circle().strokeBorder(chosen ? Color.primary.opacity(0.6) : .clear, lineWidth: 2))
                Text(name).font(.caption).foregroundStyle(chosen ? .primary : .secondary).lineLimit(1)
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(name)
        .accessibilityAddTraits(chosen ? .isSelected : [])
    }
}
