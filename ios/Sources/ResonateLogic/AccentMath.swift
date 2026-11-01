import Foundation

#if canImport(CoreGraphics)
import CoreGraphics
#endif

public enum AccentMath {
    public enum Ink: Equatable, Sendable {
        case white
        case dark(white: CGFloat)
    }

    public static let deepenedInLight: CGFloat = 0.28
    public static let darkInkWhite: CGFloat = 0.08
    public static let darkInkLuminance: CGFloat = 0.0067

    public static func rgb(_ hex: UInt32) -> (red: CGFloat, green: CGFloat, blue: CGFloat) {
        let channel = { (shift: UInt32) in CGFloat((hex >> shift) & 0xFF) / 255 }
        return (channel(16), channel(8), channel(0))
    }

    public typealias Channels = (red: CGFloat, green: CGFloat, blue: CGFloat)

    public static let readable: CGFloat = 4.5
    public static let readableWhenIncreased: CGFloat = 7
    public static let white: Channels = (1, 1, 1)
    public static let darkBackground: Channels = (0.11, 0.11, 0.12)

    public enum Statement: CaseIterable, Sendable {
        case failure
        case warning
        case confirmation

        public func ink(dark: Bool) -> Channels {
            switch (self, dark) {
            case (.failure, true): (1, 0.5, 0.47)
            case (.failure, false): (0.72, 0.1, 0.1)
            case (.warning, true): (1, 0.65, 0.2)
            case (.warning, false): (0.62, 0.3, 0)
            case (.confirmation, true): (0.4, 0.85, 0.5)
            case (.confirmation, false): (0.1, 0.45, 0.2)
            }
        }
    }

    public static func worn(_ hex: UInt32, dark: Bool, increased: Bool = false) -> Channels {
        let wanted = increased ? readableWhenIncreased : readable
        let accent = rgb(hex)
        return dark ? lifted(accent, toward: darkBackground, atLeast: wanted) : deepened(accent, atLeast: wanted)
    }

    public static func contrast(_ first: Channels, _ second: Channels) -> CGFloat {
        let one = luminance(red: first.red, green: first.green, blue: first.blue)
        let other = luminance(red: second.red, green: second.green, blue: second.blue)
        return (max(one, other) + 0.05) / (min(one, other) + 0.05)
    }

    public static func inkContrast(on hex: UInt32, dark: Bool, increased: Bool = false) -> CGFloat {
        let worn = worn(hex, dark: dark, increased: increased)
        let lettering: Channels =
            switch ink(on: hex, dark: dark, increased: increased) {
            case .white: white
            case .dark(let grey): (grey, grey, grey)
            }
        return contrast(worn, lettering)
    }

    private static func deepened(_ accent: Channels, atLeast wanted: CGFloat) -> Channels {
        let deepened = scaled(accent, by: 1 - deepenedInLight)
        guard contrast(deepened, white) < wanted else { return deepened }
        var kept: CGFloat = 0
        var lost: CGFloat = 1
        for _ in 0..<24 {
            let middle = (kept + lost) / 2
            if contrast(scaled(deepened, by: middle), white) >= wanted { kept = middle } else { lost = middle }
        }
        return scaled(deepened, by: kept)
    }

    private static func lifted(_ accent: Channels, toward background: Channels, atLeast wanted: CGFloat) -> Channels {
        guard contrast(accent, background) < wanted else { return accent }
        var kept: CGFloat = 1
        var lost: CGFloat = 0
        for _ in 0..<24 {
            let middle = (kept + lost) / 2
            if contrast(mixed(accent, with: white, by: middle), background) >= wanted { kept = middle } else { lost = middle }
        }
        return mixed(accent, with: white, by: kept)
    }

    private static func scaled(_ colour: Channels, by factor: CGFloat) -> Channels {
        (colour.red * factor, colour.green * factor, colour.blue * factor)
    }

    private static func mixed(_ colour: Channels, with other: Channels, by share: CGFloat) -> Channels {
        (
            colour.red + (other.red - colour.red) * share,
            colour.green + (other.green - colour.green) * share,
            colour.blue + (other.blue - colour.blue) * share
        )
    }

    public static func luminance(red: CGFloat, green: CGFloat, blue: CGFloat) -> CGFloat {
        func linear(_ channel: CGFloat) -> CGFloat {
            channel <= 0.04045 ? channel / 12.92 : pow((channel + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)
    }

    public static func ink(on hex: UInt32, dark: Bool, increased: Bool = false) -> Ink {
        let worn = worn(hex, dark: dark, increased: increased)
        let luminance = luminance(red: worn.red, green: worn.green, blue: worn.blue)
        let lightContrast = (1 + 0.05) / (luminance + 0.05)
        let darkContrast = (luminance + 0.05) / (darkInkLuminance + 0.05)
        return lightContrast >= darkContrast ? .white : .dark(white: darkInkWhite)
    }
}
