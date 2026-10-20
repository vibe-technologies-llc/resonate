import Foundation
import Testing

@testable import ResonateLogic

@Suite struct ErasureTests {
    let scratch = Scratch()
    let defaults = Defaults(2)

    var support: URL { scratch.url.appendingPathComponent("support") }

    func place(_ name: String) -> URL {
        scratch.url.appendingPathComponent(name)
    }

    func erase(
        keeping names: [String] = [],
        places: [URL],
        secrets: @autoclosure () -> Int = 0
    ) -> ErasureLeftovers {
        Erasure.everything(
            keeping: names,
            support: support,
            places: places,
            suites: defaults.names,
            bundle: nil,
            secrets: secrets
        )
    }

    @Test func everythingInThePlacesIsRemovedButThePlacesThemselvesStay() {
        scratch.write("caches/a.bin")
        scratch.write("caches/deep/b.bin")
        scratch.write("documents/song.flac")

        let left = erase(places: [place("caches"), place("documents")])

        #expect(left.isNone)
        #expect(FileManager.default.fileExists(atPath: place("caches").path))
        #expect(!scratch.exists("caches/a.bin"))
        #expect(!scratch.exists("caches/deep"))
        #expect(!scratch.exists("documents/song.flac"))
    }

    @Test func aPlaceThatIsNotThereIsNothingToRemove() {
        let left = erase(places: [place("never-made")])

        #expect(left.isNone)
    }

    @Test func aFileThatCannotBeRemovedIsCountedAndTheRestStillGo() throws {
        scratch.write("held/stuck.bin")
        scratch.write("free/gone.bin")
        try FileManager.default.setAttributes([.posixPermissions: 0o555], ofItemAtPath: place("held").path)
        defer { try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: place("held").path) }
        guard !FileManager.default.isWritableFile(atPath: place("held").path) else { return }

        let left = erase(places: [place("held"), place("free")])

        #expect(left.files == 1)
        #expect(!left.isNone)
        #expect(scratch.exists("held/stuck.bin"))
        #expect(!scratch.exists("free/gone.bin"))
    }

    @Test func aPlaceThatCannotBeListedIsCountedOnce() throws {
        scratch.write("sealed/a.bin")
        try FileManager.default.setAttributes([.posixPermissions: 0o000], ofItemAtPath: place("sealed").path)
        defer { try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: place("sealed").path) }
        guard (try? FileManager.default.contentsOfDirectory(atPath: place("sealed").path)) == nil else { return }

        let left = erase(places: [place("sealed")])

        #expect(left.files == 1)
    }

    @Test func theNamedFilesAreKeptThroughTheErasureWhereTheSupportFolderWasEmptied() {
        scratch.write("support/folders.plist", "bookmarks")
        scratch.write("support/library.db", "catalog")

        let left = erase(keeping: ["folders.plist", "absent.plist"], places: [support])

        #expect(left.isNone)
        #expect(scratch.read("support/folders.plist") == "bookmarks")
        #expect(!scratch.exists("support/library.db"))
        #expect(!scratch.exists("support/absent.plist"))
    }

    @Test func theNamedFilesComeBackEvenWhereTheSupportFolderWasRemovedWhole() {
        scratch.write("support/folders.plist", "bookmarks")
        scratch.write("support/library.db", "catalog")

        _ = erase(keeping: ["folders.plist"], places: [support])

        #expect(scratch.read("support/folders.plist") == "bookmarks")
    }

    @Test func everySuiteIsEmptied() {
        for name in defaults.names {
            UserDefaults(suiteName: name)!.set("x", forKey: "key")
            UserDefaults(suiteName: name)!.synchronize()
        }

        _ = erase(places: [])

        for name in defaults.names { #expect(UserDefaults(suiteName: name)!.object(forKey: "key") == nil) }
    }

    @Test func theSecretsLeftAreWhatTheKeychainReportsAndNeedNoFilesToBeLeft() {
        let left = erase(places: [], secrets: 2)

        #expect(left.secrets == 2)
        #expect(!left.isNone)
        #expect(left.sentences == ["2 saved keys could not be deleted."])
    }

    @Test func leftoversAreToldOneSentenceForEachKindThatRemains() {
        let left = ErasureLeftovers(songs: 1, files: 3, secrets: 1)

        #expect(
            left.sentences == [
                "1 song could not be deleted and stay in their folders, which Resonate keeps.",
                "3 files of the app's own could not be deleted.",
                "1 saved key could not be deleted.",
            ]
        )
        #expect(ErasureLeftovers().sentences.isEmpty)
        #expect(ErasureLeftovers().isNone)
    }

    @Test func everySuiteTheAppUsesIsErased() throws {
        let sources = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("Sources")
        let pattern = try NSRegularExpression(pattern: #"(?:suiteName:|[sS]uite\s*=)\s*"([a-z][a-z-]*)""#)

        var used: Set<String> = []
        for folder in ["Resonate", "ResonateLogic"] {
            let swift = try FileManager.default.contentsOfDirectory(
                at: sources.appendingPathComponent(folder),
                includingPropertiesForKeys: nil
            ).filter { $0.pathExtension == "swift" }
            for file in swift {
                let text = try String(contentsOf: file, encoding: .utf8)
                let range = NSRange(text.startIndex..., in: text)
                for match in pattern.matches(in: text, range: range) {
                    used.insert(String(text[Range(match.range(at: 1), in: text)!]))
                }
            }
        }

        #expect(used.count >= 4)
        #expect(used.isSubset(of: Set(Erasure.suites)), "\(used.subtracting(Erasure.suites))")
    }
}
