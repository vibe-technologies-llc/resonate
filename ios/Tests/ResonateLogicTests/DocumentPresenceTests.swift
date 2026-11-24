import Foundation
import Testing

@testable import ResonateLogic

@Suite struct DocumentPresenceTests {
    let scratch = Scratch()

    var root: URL { scratch.url.appendingPathComponent("music") }
    var song: URL { root.appendingPathComponent("Album/a.flac") }

    @Test func aSongGoneFromAFolderThatListsIsNotFound() {
        scratch.write("music/Album/b.flac")

        #expect(DocumentPresence.of(song, under: root) == .absent)
        #expect(OpenFailure.opening(song, under: root, failedWith: ENOENT) == .notFound)
    }

    @Test func aFolderGoneFromAFolderThatListsTakesItsSongsWithIt() {
        scratch.write("music/Other/b.flac")

        #expect(DocumentPresence.of(song, under: root) == .absent)
        #expect(OpenFailure.opening(song, under: root, failedWith: ENOENT) == .notFound)
    }

    @Test func aFolderWhoseBookmarkNoLongerReachesItLeavesItsSongsRefused() {
        let unplugged = scratch.url.appendingPathComponent("unplugged")
        let lost = unplugged.appendingPathComponent("Album/a.flac")

        #expect(DocumentPresence.of(lost, under: unplugged) == .unknown)
        #expect(OpenFailure.opening(lost, under: unplugged, failedWith: ENOENT) == .refused)
    }

    @Test func anUnavailableSubfolderLeavesItsSongsRefused() throws {
        scratch.write("music/Album/a.flac")
        let album = root.appendingPathComponent("Album")
        try FileManager.default.setAttributes([.posixPermissions: 0o000], ofItemAtPath: album.path)
        defer { try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: album.path) }

        let offline = DocumentPresence.of(song, under: root) { $0.path == album.path ? nil : ["Album"] }

        #expect(offline == .unknown)
        #expect(DocumentPresence.of(song, under: root) == .unknown)
        #expect(OpenFailure.opening(song, under: root, failedWith: ENOENT) == .refused)
    }

    @Test func aProviderThatListsNothingLeavesEverySongRefused() {
        let presence = DocumentPresence.of(song, under: root) { _ in nil }

        #expect(presence == .unknown)
        #expect(OpenFailure.after(POSIXError(.ENOENT), presence: { presence }) == .refused)
    }

    @Test func aSongTheFolderStillListsIsRefusedThoughItsOpenSaidNoSuchFile() {
        scratch.write("music/Album/a.flac")

        #expect(DocumentPresence.of(song, under: root) == .standing)
        #expect(OpenFailure.opening(song, under: root, failedWith: ENOENT) == .refused)
    }

    @Test func onlyNoSuchFileAsksWhereTheSongIs() {
        var asked = 0
        let failures: [any Swift.Error] = [
            POSIXError(.EACCES),
            POSIXError(.EINTR),
            POSIXError(.EIO),
            NSError(domain: NSCocoaErrorDomain, code: Int(POSIXErrorCode.ENOENT.rawValue)),
        ]

        let answers = failures.map { failure in
            OpenFailure.after(failure) {
                asked += 1
                return .absent
            }
        }

        #expect(answers.allSatisfy { $0 == .refused })
        #expect(asked == 0)
    }

    @Test func aSongOutsideItsFolderIsNeverAbsent() {
        #expect(DocumentPresence.of(scratch.url.appendingPathComponent("a.flac"), under: root) == .unknown)
        #expect(DocumentPresence.of(root, under: root) == .unknown)
    }
}
