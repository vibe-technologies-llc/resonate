import Foundation
import Testing

@testable import ResonateLogic

@Suite struct FileRemovalTests {
    let scratch = Scratch()

    @Test func aFileRemovedOrAlreadyMissingUnderAReachableFolderIsGone() {
        scratch.write("music/song.flac")
        let folder = scratch.url.appendingPathComponent("music")

        let removed = FileRemoval.removed(folder.appendingPathComponent("song.flac"), under: folder)
        let missing = FileRemoval.removed(folder.appendingPathComponent("never.flac"), under: folder)

        #expect(removed)
        #expect(missing)
        #expect(!scratch.exists("music/song.flac"))
    }

    @Test func aFileUnderAFolderThatCannotBeReachedIsNeverGone() {
        let unplugged = scratch.url.appendingPathComponent("unplugged")

        let gone = FileRemoval.removed(unplugged.appendingPathComponent("song.flac"), under: unplugged)

        #expect(!gone)
    }

    @Test func aFileTheFolderRefusesToGiveUpStays() throws {
        scratch.write("locked/song.flac")
        let folder = scratch.url.appendingPathComponent("locked")
        try FileManager.default.setAttributes([.posixPermissions: 0o555], ofItemAtPath: folder.path)
        defer { try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: folder.path) }

        let gone = FileRemoval.removed(folder.appendingPathComponent("song.flac"), under: folder)

        #expect(!gone)
        #expect(scratch.exists("locked/song.flac"))
    }

    @Test func onlyNoSuchFileUnderAReachableFolderCountsAsGone() {
        let missing = CocoaError(.fileNoSuchFile)
        let missingBeneath = NSError(
            domain: NSCocoaErrorDomain,
            code: CocoaError.Code.fileWriteUnknown.rawValue,
            userInfo: [NSUnderlyingErrorKey: POSIXError(.ENOENT) as NSError]
        )

        #expect(FileRemoval.isGone(after: missing, folderReachable: true))
        #expect(FileRemoval.isGone(after: POSIXError(.ENOENT), folderReachable: true))
        #expect(FileRemoval.isGone(after: missingBeneath, folderReachable: true))
        #expect(!FileRemoval.isGone(after: missing, folderReachable: false))
        #expect(!FileRemoval.isGone(after: CocoaError(.fileWriteNoPermission), folderReachable: true))
        #expect(!FileRemoval.isGone(after: POSIXError(.EIO), folderReachable: true))
        #expect(!FileRemoval.isGone(after: POSIXError(.EPERM), folderReachable: true))
    }

    @Test func aCodeThatMeansNoSuchFileOnlyInAnotherDomainIsNeverGone() {
        let interrupted = POSIXError(.EINTR)
        let cocoaNumberedAsNoSuchFile = NSError(
            domain: NSCocoaErrorDomain,
            code: Int(POSIXErrorCode.ENOENT.rawValue)
        )
        let interruptedBeneath = NSError(
            domain: NSCocoaErrorDomain,
            code: CocoaError.Code.fileWriteUnknown.rawValue,
            userInfo: [NSUnderlyingErrorKey: interrupted as NSError]
        )

        #expect(Int(POSIXErrorCode.EINTR.rawValue) == CocoaError.Code.fileNoSuchFile.rawValue)
        #expect(!FileRemoval.isGone(after: interrupted, folderReachable: true))
        #expect(!FileRemoval.isGone(after: cocoaNumberedAsNoSuchFile, folderReachable: true))
        #expect(!FileRemoval.isGone(after: interruptedBeneath, folderReachable: true))
        #expect(FileRemoval.isGone(after: POSIXError(.ENOENT), folderReachable: true))
        #expect(FileRemoval.isGone(after: CocoaError(.fileNoSuchFile), folderReachable: true))
    }
}
