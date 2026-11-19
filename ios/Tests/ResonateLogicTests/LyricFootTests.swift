import Testing

@testable import ResonateLogic

@Suite struct LyricFootTests {
    private func near(_ found: [Double], _ wanted: [Double]) -> Bool {
        found.count == wanted.count && zip(found, wanted).allSatisfy { abs($0 - $1) < 1e-9 }
    }

    @Test func aPaneWithNoFootAimsAndFadesAsItAlwaysDid() {
        let foot = LyricFoot(height: 400, foot: 0)

        #expect(foot.aimed == 400)
        #expect(near(foot.fade(dissolve: 40, clear: false), [0, 0.1, 0.9, 1]))
    }

    @Test func theLinesFadeAboveTheControlsUntilTheyGoAway() {
        let foot = LyricFoot(height: 400, foot: 100)

        #expect(foot.aimed == 300)
        #expect(near(foot.fade(dissolve: 40, clear: false), [0, 0.1, 0.65, 0.75]))
        #expect(near(foot.fade(dissolve: 40, clear: true), [0, 0.1, 0.9, 1]))
    }

    @Test func aFootTallerThanThePaneLeavesNothingToAimAt() {
        let foot = LyricFoot(height: 200, foot: 500)

        #expect(foot.aimed == 0)
        #expect(near(foot.fade(dissolve: 40, clear: false), [0, 0.2, 0.2, 0.2]))
        #expect(LyricFoot(height: 0, foot: 10).fade(dissolve: 40, clear: false) == [0, 0, 1, 1])
        #expect(LyricFoot(height: 300, foot: -20).aimed == 300)
    }
}
