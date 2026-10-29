import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct MeasurementTests {
    let flat = BuiltInTarget(name: "flat", label: "Flat", recommended: false)
    let overEar = BuiltInTarget(name: "harman-over-ear-2018", label: "Harman over-ear 2018", recommended: true)
    let inEar = BuiltInTarget(name: "harman-in-ear-2019", label: "Harman in-ear 2019", recommended: false)

    var offers: MeasurementOffers {
        MeasurementOffers(targets: [flat, overEar, inEar], bytesAtMost: 4_194_304)
    }

    func fit(_ bands: UInt32, _ before: Float, _ after: Float, passedOver: UInt32 = 0, clamped: UInt32 = 0) -> MeasurementFit {
        MeasurementFit(
            imported: EqualiserImport(
                curve: EqualiserCurve(name: "Mine", preampDb: -3, bands: []),
                passedOver: passedOver,
                approximated: 0,
                clamped: clamped,
                converted: false
            ),
            bands: bands,
            offByDbBefore: before,
            offByDbAfter: after
        )
    }

    @Test func theRecommendedBuiltInIsAimedAtFirst() {
        #expect(MeasurementView.of(offers).aim == .builtIn(name: "harman-over-ear-2018"))
        #expect(
            MeasurementView.of(MeasurementOffers(targets: [flat, inEar], bytesAtMost: 1)).aim == .builtIn(name: "flat")
        )
        #expect(MeasurementView.of(MeasurementOffers(targets: [], bytesAtMost: 1)).aim == nil)
    }

    @Test func aBuiltInIsAskedForByNameAndADocumentByItsBytes() {
        let bytes = Data([49, 48, 48])

        #expect(MeasurementAim.builtIn(name: "flat").target == .builtIn(name: "flat"))
        #expect(MeasurementAim.document(name: "mine.csv", bytes: bytes).target == .document(bytes: bytes))
    }

    @Test func anAimIsLabelledByTheBuiltInsLabelOrTheFilesName() {
        let targets = offers.targets

        #expect(MeasurementAim.builtIn(name: "harman-in-ear-2019").label(among: targets) == "Harman in-ear 2019")
        #expect(MeasurementAim.builtIn(name: "unknown").label(among: targets) == "unknown")
        #expect(MeasurementAim.document(name: "target.csv", bytes: Data()).label(among: targets) == "target.csv")
    }

    @Test func aProfileIsNamedAfterTheMeasurementFileWithoutItsExtension() {
        #expect(Measurement.profileName(for: "HD 650.csv") == "HD 650")
        #expect(Measurement.profileName(for: "left.ear.txt") == "left.ear")
        #expect(Measurement.profileName(for: "sweep") == "sweep")
    }

    @Test func theFitIsToldWithItsBandsItsTargetAndTheErrorBeforeAndAfter() {
        #expect(
            Measurement.told(fit(7, 5.14, 1.2), toward: "Harman over-ear 2018")
                == "Fitted 7 bands toward Harman over-ear 2018: off by 5.1 dB before, 1.2 dB after"
        )
        #expect(
            Measurement.told(fit(1, 2, 0.5), toward: "Flat") == "Fitted 1 band toward Flat: off by 2.0 dB before, 0.5 dB after"
        )
    }

    @Test func linesPassedOverAndValuesHeldToTheLimitsAreToldOnlyWhenThereWereSome() {
        let said = Measurement.told(fit(3, 2, 1, passedOver: 1, clamped: 2), toward: "Flat")

        #expect(said.hasSuffix(" · 1 line of the file passed over · 2 values held to the limits"))
        #expect(!Measurement.told(fit(3, 2, 1), toward: "Flat").contains("passed over"))
        #expect(!Measurement.told(fit(3, 2, 1), toward: "Flat").contains("held"))
    }

    @Test func aFitInProgressClearsTheLastSentenceAndKeepsTheNewOneWhenItEnds() {
        let earlier = MeasurementView.of(offers).finished(saying: "Fitted earlier")

        let begun = earlier.begun()
        let done = begun.finished(saying: "Fitted now")
        let failed = begun.finished(saying: nil)

        #expect(earlier.said == "Fitted earlier")
        #expect(begun.fitting)
        #expect(begun.said == nil)
        #expect(!done.fitting)
        #expect(done.said == "Fitted now")
        #expect(!failed.fitting)
        #expect(failed.said == nil)
    }

    @Test func aRoomMeasuredSoundsThenFitsAndEndsWithItsSentenceOrNone() {
        let earlier = MeasurementView.of(offers).finished(saying: "Fitted earlier").turnedAway()

        let sounding = earlier.sounded()
        let fitting = sounding.heard()
        let done = fitting.finished(saying: "Fitted now")
        let stopped = sounding.finished(saying: nil)

        #expect(sounding.sounding && sounding.fitting)
        #expect(sounding.said == nil)
        #expect(!sounding.denied)
        #expect(!fitting.sounding && fitting.fitting)
        #expect(!done.fitting && !done.sounding)
        #expect(done.said == "Fitted now")
        #expect(!stopped.fitting && !stopped.sounding)
        #expect(stopped.said == nil)
    }

    @Test func aMicrophoneRefusedLeavesTheMeasurementIdleAndSaysSoApartFromTheSentence() {
        let refused = MeasurementView.of(offers).sounded().turnedAway()

        #expect(refused.denied)
        #expect(!refused.fitting && !refused.sounding)
        #expect(refused.said == nil)
        #expect(!refused.sounded().denied)
    }

    @Test func aCalibrationIsKeptUntilForgottenAndSurvivesAMeasurement() {
        let calibration = MeasurementCalibration(name: "mic.csv", bytes: Data([1]))
        let view = MeasurementView.of(offers).calibrated(by: calibration)

        #expect(view.sounded().finished(saying: "done").calibration == calibration)
        #expect(view.calibrated(by: nil).calibration == nil)
    }
}
