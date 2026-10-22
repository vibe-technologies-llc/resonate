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

    public static func worn(_ hex: UInt32, dark: Bool) -> (red: CGFloat, green: CGFloat, blue: CGFloat) {
        let worn = rgb(hex)
        let deepened = dark ? 0 : deepenedInLight
        return (worn.red * (1 - deepened), worn.green * (1 - deepened), worn.blue * (1 - deepened))
    }

    public static func luminance(red: CGFloat, green: CGFloat, blue: CGFloat) -> CGFloat {
        func linear(_ channel: CGFloat) -> CGFloat {
            channel <= 0.04045 ? channel / 12.92 : pow((channel + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)
    }

    public static func ink(on hex: UInt32, dark: Bool) -> Ink {
        let worn = worn(hex, dark: dark)
        let luminance = luminance(red: worn.red, green: worn.green, blue: worn.blue)
        let lightContrast = (1 + 0.05) / (luminance + 0.05)
        let darkContrast = (luminance + 0.05) / (darkInkLuminance + 0.05)
        return lightContrast >= darkContrast ? .white : .dark(white: darkInkWhite)
    }
}
