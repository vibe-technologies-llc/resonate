import Foundation

public enum RoomPair {
    public static func name(left: String, right: String) -> String {
        "\(stem(of: left)) and \(stem(of: right))"
    }

    public static func stem(of file: String) -> String {
        guard let dot = file.lastIndex(of: ".") else { return file }
        return String(file[..<dot])
    }

    public static func fileExtension(of file: String) -> String {
        guard let dot = file.lastIndex(of: ".") else { return "" }
        return String(file[file.index(after: dot)...])
    }
}
