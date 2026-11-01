import Foundation

public enum HomeGreeting {
    public static func of(hour: Int) -> String {
        switch hour {
        case 5..<12: "Good morning"
        case 12..<18: "Good afternoon"
        default: "Good evening"
        }
    }
}
