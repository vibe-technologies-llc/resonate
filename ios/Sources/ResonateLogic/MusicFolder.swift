public struct MusicFolder: Identifiable, Hashable, Sendable {
    public let id: String
    public let name: String
    public let path: String
    public let readable: Bool
    public let writable: Bool

    public init(id: String, name: String, path: String, readable: Bool, writable: Bool) {
        self.id = id
        self.name = name
        self.path = path
        self.readable = readable
        self.writable = writable
    }
}
