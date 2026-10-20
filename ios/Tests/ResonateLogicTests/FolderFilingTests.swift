import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

final class Scratch {
    let url: URL

    init() {
        url = FileManager.default.temporaryDirectory
            .appendingPathComponent("resonate-logic-\(UUID().uuidString)", isDirectory: true)
        try! FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    }

    deinit {
        try? FileManager.default.removeItem(at: url)
    }

    func write(_ relative: String, _ text: String = "audio") {
        let file = url.appendingPathComponent(relative)
        try! FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try! Data(text.utf8).write(to: file)
    }

    func exists(_ relative: String) -> Bool {
        FileManager.default.fileExists(atPath: url.appendingPathComponent(relative).path)
    }

    func read(_ relative: String) -> String? {
        try? String(contentsOf: url.appendingPathComponent(relative), encoding: .utf8)
    }
}

@Suite struct FolderURITests {
    @Test func aRelativePathIsEscapedPieceByPieceAndKeepsItsSlashes() {
        let uri = FolderURI.make(root: "abc", relative: "Pink Floyd/100% #1?.flac")

        #expect(uri == "resonate-folder://abc/Pink%20Floyd/100%25%20%231%3F.flac")
    }

    @Test func aMadeUriIsReadBackToTheSamePieces() {
        let uri = FolderURI.make(root: "abc", relative: "Björk/Homogenic/01 Hunter.flac")

        let (root, relative) = FolderURI.split(uri)!

        #expect(root == "abc")
        #expect(FolderURI.components(of: relative) == ["Björk", "Homogenic", "01 Hunter.flac"])
    }

    @Test func aUriNamingOnlyTheRootHasNoRelativePath() {
        let (root, relative) = FolderURI.split("resonate-folder://local")!

        #expect(root == "local")
        #expect(relative == "")
        #expect(FolderURI.components(of: relative) == [])
    }

    @Test func aUriOfAnotherSchemeOrNoneIsNotAFolderUri() {
        #expect(FolderURI.split("file:///music/a.flac") == nil)
        #expect(FolderURI.split("/music/a.flac") == nil)
        #expect(FolderURI.root(of: "content://x/a.flac") == nil)
    }

    @Test func aRootIsTheFirstPieceAfterTheScheme() {
        #expect(FolderURI.root(of: "resonate-folder://local/a/b.flac") == "local")
        #expect(FolderURI.root(of: "resonate-folder://abc") == "abc")
    }

    @Test(arguments: ["../secret.flac", "a/../../secret.flac", "a/%2e%2e/secret.flac", "%2E%2E/x"])
    func aPathClimbingOutOfItsRootIsRefused(relative: String) {
        #expect(FolderURI.components(of: relative) == nil)
    }

    @Test func aDotsNameThatIsNotAClimbIsAllowed() {
        #expect(FolderURI.components(of: "a../b..c/..d.flac") == ["a..", "b..c", "..d.flac"])
    }
}

@Suite struct FolderFilingTests {
    final class Released: @unchecked Sendable {
        private let lock = NSLock()
        private var count = 0

        func note() { lock.withLock { count += 1 } }
        var times: Int { lock.withLock { count } }
    }

    let scratch = Scratch()
    let released = Released()

    func resolving(_ uri: String) throws -> ResolvedDocument {
        guard let (_, relative) = FolderURI.split(uri), let path = FolderURI.components(of: relative) else {
            throw CocoaError(.fileReadInvalidFileName)
        }
        let url = path.reduce(scratch.url) { $0.appendingPathComponent($1) }
        return ResolvedDocument(url: url) { released.note() }
    }

    func uri(_ relative: String) -> String {
        FolderURI.make(root: "abc", relative: relative)
    }

    func file(_ moves: [FiledSong]) -> FiledMoves {
        FolderFiling.file(moves, in: "abc", at: scratch.url, resolving: resolving)
    }

    @Test func aSongIsMovedToItsPlaceMakingTheFoldersAboveIt() {
        scratch.write("loose/a.flac", "one")

        let filed = file([FiledSong(id: 1, from: uri("loose/a.flac"), to: ["Artist", "Album", "01 A.flac"])])

        #expect(filed.failed == 0)
        #expect(scratch.read("Artist/Album/01 A.flac") == "one")
        #expect(!scratch.exists("loose/a.flac"))
        #expect(filed.moved == [MovedDocument(id: 1, uri: uri("Artist/Album/01 A.flac"))])
    }

    @Test func theMovesBackNameTheOldPlaceSoFilingThemRestoresIt() {
        scratch.write("loose/deep/a.flac", "one")
        let there = file([FiledSong(id: 1, from: uri("loose/deep/a.flac"), to: ["Artist", "01 A.flac"])])
        #expect(there.back == [FiledSong(id: 1, from: uri("Artist/01 A.flac"), to: ["loose", "deep", "a.flac"])])

        let back = file(there.back)

        #expect(back.failed == 0)
        #expect(scratch.read("loose/deep/a.flac") == "one")
        #expect(!scratch.exists("Artist"))
    }

    @Test func aSongWhoseDestinationIsTakenIsLeftWhereItIsAndCountedFailed() {
        scratch.write("loose/a.flac", "one")
        scratch.write("Artist/01 A.flac", "another")

        let filed = file([FiledSong(id: 1, from: uri("loose/a.flac"), to: ["Artist", "01 A.flac"])])

        #expect(filed.failed == 1)
        #expect(filed.moved.isEmpty)
        #expect(scratch.read("loose/a.flac") == "one")
        #expect(scratch.read("Artist/01 A.flac") == "another")
    }

    @Test func aSongThatCannotBeResolvedIsCountedFailedAndTheRestAreStillFiled() {
        scratch.write("loose/b.flac", "two")

        let filed = file([
            FiledSong(id: 1, from: "file:///elsewhere/a.flac", to: ["x.flac"]),
            FiledSong(id: 2, from: uri("loose/b.flac"), to: ["y.flac"]),
        ])

        #expect(filed.failed == 1)
        #expect(filed.moved.map(\.id) == [2])
        #expect(scratch.read("y.flac") == "two")
    }

    @Test func aSongOutsideTheRootIsNeverMoved() throws {
        let outside = Scratch()
        outside.write("a.flac", "outside")
        let there = outside.url.appendingPathComponent("a.flac")

        let filed = FolderFiling.file(
            [FiledSong(id: 1, from: "anything", to: ["in.flac"])],
            in: "abc",
            at: scratch.url
        ) { _ in ResolvedDocument(url: there) { self.released.note() } }

        #expect(filed.failed == 1)
        #expect(outside.read("a.flac") == "outside")
        #expect(!scratch.exists("in.flac"))
    }

    @Test func aRootsSiblingSharingItsNamePrefixIsNotInsideIt() {
        let sibling = scratch.url.deletingLastPathComponent()
            .appendingPathComponent(scratch.url.lastPathComponent + "-more", isDirectory: true)
        try! FileManager.default.createDirectory(at: sibling, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: sibling) }
        try! Data("x".utf8).write(to: sibling.appendingPathComponent("a.flac"))

        let filed = FolderFiling.file(
            [FiledSong(id: 1, from: "anything", to: ["in.flac"])],
            in: "abc",
            at: scratch.url
        ) { _ in ResolvedDocument(url: sibling.appendingPathComponent("a.flac")) {} }

        #expect(filed.failed == 1)
        #expect(!scratch.exists("in.flac"))
    }

    @Test func aSidecarLyricSheetMovesWithItsSongWhereItsPlaceIsFree() {
        scratch.write("loose/a.flac", "one")
        scratch.write("loose/a.lrc", "[00:01.00]words")

        _ = file([FiledSong(id: 1, from: uri("loose/a.flac"), to: ["Artist", "01 A.flac"])])

        #expect(scratch.read("Artist/01 A.lrc") == "[00:01.00]words")
        #expect(!scratch.exists("loose/a.lrc"))
    }

    @Test func aSidecarIsLeftWhereItsDestinationAlreadyHoldsOne() {
        scratch.write("loose/a.flac", "one")
        scratch.write("loose/a.lrc", "mine")
        scratch.write("Artist/01 A.lrc", "theirs")

        _ = file([FiledSong(id: 1, from: uri("loose/a.flac"), to: ["Artist", "01 A.flac"])])

        #expect(scratch.read("Artist/01 A.lrc") == "theirs")
        #expect(scratch.read("loose/a.lrc") == "mine")
    }

    @Test func foldersTheMovesEmptiedAreRemovedUpToButNeverIncludingTheRoot() {
        scratch.write("a/b/c/song.flac", "one")
        scratch.write("a/b/c/.DS_Store", "")

        _ = file([FiledSong(id: 1, from: uri("a/b/c/song.flac"), to: ["song.flac"])])

        #expect(!scratch.exists("a"))
        #expect(scratch.exists("song.flac"))
        #expect(FileManager.default.fileExists(atPath: scratch.url.path))
    }

    @Test func aFolderStillHoldingAnythingElseIsKeptAndSoAreItsParents() {
        scratch.write("a/b/song.flac", "one")
        scratch.write("a/b/cover.jpg", "picture")

        _ = file([FiledSong(id: 1, from: uri("a/b/song.flac"), to: ["song.flac"])])

        #expect(scratch.exists("a/b/cover.jpg"))
    }

    @Test func everyResolvedSourceIsReleasedWhetherOrNotItMoved() {
        scratch.write("loose/a.flac", "one")
        scratch.write("loose/b.flac", "two")
        scratch.write("taken.flac", "x")

        _ = file([
            FiledSong(id: 1, from: uri("loose/a.flac"), to: ["a.flac"]),
            FiledSong(id: 2, from: uri("loose/b.flac"), to: ["taken.flac"]),
        ])

        #expect(released.times == 2)
    }
}
