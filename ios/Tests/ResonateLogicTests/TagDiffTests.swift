import Testing

@testable import ResonateLogic

@Suite struct TagDiffTests {
    @Test func aMixedFieldLeftBlankIsUnchangedAndAnEmptiedOneIsWrittenEmpty() {
        let mixed: Set<TagName> = [.genre]

        let left = TagDiff.changed(shown: [:], read: [:], mixed: mixed)
        let emptied = TagDiff.changed(shown: [:], read: [:], mixed: mixed, emptied: [.genre])

        #expect(left.isEmpty)
        #expect(emptied == [.genre])
        #expect(TagDiff.change(shown: [:], changed: emptied).genre == "")
    }

    @Test func aMixedFieldTypedInIsWrittenWhetherOrNotItWasEmptied() {
        let changed = TagDiff.changed(shown: [.genre: "Jazz"], read: [:], mixed: [.genre], emptied: [.genre])

        #expect(changed == [.genre])
        #expect(TagDiff.change(shown: [.genre: "Jazz"], changed: changed).genre == "Jazz")
    }
}
