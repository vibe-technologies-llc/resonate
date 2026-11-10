import Foundation

public enum HomeGreeting {
    public static func of(hour: Int) -> String {
        switch hour {
        case 5..<12: said("home.greeting.morning")
        case 12..<18: said("home.greeting.afternoon")
        default: said("home.greeting.evening")
        }
    }
}
