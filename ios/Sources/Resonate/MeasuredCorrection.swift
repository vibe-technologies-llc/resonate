import Foundation
import Observation
import ResonateLogic
import ResonateNative
import SwiftUI
import UniformTypeIdentifiers

@MainActor
@Observable
final class MeasuredCorrectionState {
    @ObservationIgnored private unowned let library: Library
    @ObservationIgnored private let adopt: @MainActor (EqualiserImport) -> Void

    @ObservationIgnored private var measuring: Task<Void, Never>?

    nonisolated private static let measuredName = "Measured"

    private(set) var view = MeasurementView.of(measurementOffers())

    init(library: Library, adopt: @escaping @MainActor (EqualiserImport) -> Void) {
        self.library = library
        self.adopt = adopt
    }

    func aim(at aim: MeasurementAim) {
        view = view.aimed(at: aim)
    }

    func aimAtDocument(_ url: URL) {
        let name = url.lastPathComponent
        let largest = Int(view.offers.bytesAtMost)
        Task {
            do {
                let bytes = try await Work.aside { try Self.contents(of: url, atMost: largest) }
                view = view.aimed(at: .document(name: name, bytes: bytes))
            } catch {
                library.report(error)
            }
        }
    }

    func fit(_ url: URL) {
        guard !view.fitting, let aim = view.aim else { return }
        let name = Measurement.profileName(for: url.lastPathComponent)
        let largest = Int(view.offers.bytesAtMost)
        let targets = view.offers.targets
        view = view.begun()
        Task {
            do {
                let bytes = try await Work.aside { try Self.contents(of: url, atMost: largest) }
                let target = aim.target
                let fit = try await Work.aside { try fitMeasurement(measurement: bytes, target: target, name: name) }
                adopt(fit.imported)
                let said = Measurement.told(fit, toward: aim.label(among: targets))
                view = view.finished(saying: said)
                library.notice(said)
            } catch {
                view = view.finished(saying: nil)
                library.report(error)
            }
        }
    }

    func calibrate(_ url: URL) {
        let name = url.lastPathComponent
        let largest = Int(view.offers.bytesAtMost)
        Task {
            do {
                let bytes = try await Work.aside { try Self.contents(of: url, atMost: largest) }
                view = view.calibrated(by: MeasurementCalibration(name: name, bytes: bytes))
            } catch {
                library.report(error)
            }
        }
    }

    func forgetCalibration() {
        view = view.calibrated(by: nil)
    }

    func measure() {
        guard !view.fitting, let aim = view.aim else { return }
        let calibration = view.calibration?.bytes
        let targets = view.offers.targets
        view = view.sounded()
        measuring = Task {
            do {
                guard await Microphone.allowed() else {
                    view = view.turnedAway()
                    return
                }
                if library.transport == .playing || library.transport == .loading { library.pause() }
                let clip = try await Microphone.sweep(through: library.nowPlaying)
                view = view.heard()
                let target = aim.target
                let fit = try await Work.aside {
                    try fitRecordedSweep(
                        recorded: clip.samples, rate: clip.rate, calibration: calibration, target: target,
                        name: Self.measuredName
                    )
                }
                adopt(fit.imported)
                let said = Measurement.told(fit, toward: aim.label(among: targets))
                view = view.finished(saying: said)
                library.notice(said)
            } catch is CancellationError {
                view = view.finished(saying: nil)
            } catch {
                view = view.finished(saying: nil)
                library.report(error)
            }
        }
    }

    func stopMeasuring() {
        measuring?.cancel()
    }

    nonisolated private static func contents(of url: URL, atMost largest: Int) throws -> Data {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        return try handle.read(upToCount: largest + 1) ?? Data()
    }
}

struct MeasuredCorrectionSection: View {
    private static let documentTag = "\u{0}document"
    private static let fileTypes: [UTType] = [.plainText, .text, .commaSeparatedText, .data]

    @Environment(Library.self) private var library
    @State private var choosingMeasurement = false
    @State private var choosingTarget = false
    @State private var choosingCalibration = false
    @Environment(\.openURL) private var openURL

    var body: some View {
        let measured = library.features.measured
        let view = measured.view
        Section {
            Picker("Fit toward", selection: Binding(get: { Self.tag(of: view.aim) }, set: { Self.choose($0, view, measured) })) {
                ForEach(view.offers.targets, id: \.name) { target in
                    Text(target.label).tag(target.name)
                }
                if case .document(let name, _) = view.aim {
                    Text(name).tag(Self.documentTag)
                }
            }
            .disabled(view.fitting)
            Button("Choose a target file…", systemImage: "doc.text") { choosingTarget = true }
                .disabled(view.fitting)
                .fileImporter(isPresented: $choosingTarget, allowedContentTypes: Self.fileTypes) { result in
                    switch result {
                    case .success(let url): measured.aimAtDocument(url)
                    case .failure(let failure): library.report(failure)
                    }
                }
            Button(view.fitting ? "Fitting…" : "Fit a measurement…", systemImage: "waveform.path.ecg") { choosingMeasurement = true }
                .disabled(view.fitting || view.aim == nil)
                .fileImporter(isPresented: $choosingMeasurement, allowedContentTypes: Self.fileTypes) { result in
                    switch result {
                    case .success(let url): measured.fit(url)
                    case .failure(let failure): library.report(failure)
                    }
                }
            Button(view.sounding ? "Measuring…" : "Measure your room", systemImage: "mic") { measured.measure() }
                .disabled(view.fitting || view.aim == nil)
            if view.sounding {
                Button("Stop", systemImage: "stop.fill", role: .cancel) { measured.stopMeasuring() }
            }
            if let calibration = view.calibration {
                Button("Forget the calibration · \(calibration.name)", systemImage: "xmark.circle") { measured.forgetCalibration() }
                    .disabled(view.fitting)
            } else {
                Button("Microphone calibration…", systemImage: "doc.text") { choosingCalibration = true }
                    .disabled(view.fitting)
                    .fileImporter(isPresented: $choosingCalibration, allowedContentTypes: Self.fileTypes) { result in
                        switch result {
                        case .success(let url): measured.calibrate(url)
                        case .failure(let failure): library.report(failure)
                        }
                    }
            }
            if view.denied {
                Button("Resonate needs the microphone to hear the sweep. Open Settings", systemImage: "mic.slash") {
                    if let settings = URL(string: UIApplication.openSettingsURLString) { openURL(settings) }
                }
            }
            if view.fitting {
                ProgressView().frame(maxWidth: .infinity)
            }
        } header: {
            Text("Measured correction")
        } footer: {
            Text(view.said ?? "Fit a few bands to your own frequency-response measurement, as AutoEq's CSV or REW's exported text, toward a target. Measure your room pauses the music, plays a sweep through the speaker and fits what the microphone hears; the microphone is taken flat unless you give it a calibration file.")
        }
    }

    private static func tag(of aim: MeasurementAim?) -> String {
        switch aim {
        case .builtIn(let name): name
        case .document: documentTag
        case nil: ""
        }
    }

    private static func choose(_ tag: String, _ view: MeasurementView, _ measured: MeasuredCorrectionState) {
        guard tag != documentTag else { return }
        measured.aim(at: .builtIn(name: tag))
    }
}
