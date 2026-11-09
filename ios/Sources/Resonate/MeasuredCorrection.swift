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

struct MicrophoneMeasurementView: View {
    private static let documentTag = "\u{0}document"
    private static let fileTypes: [UTType] = [.plainText, .text, .commaSeparatedText, .data]

    @Environment(Library.self) private var library
    @Environment(\.openURL) private var openURL
    @State private var choosingMeasurement = false
    @State private var choosingTarget = false
    @State private var choosingCalibration = false

    var body: some View {
        let measured = library.features.measured
        let view = measured.view
        ThemedForm {
            Section {
                Picker(selection: Binding(get: { Self.tag(of: view.aim) }, set: { Self.choose($0, measured) })) {
                    ForEach(view.offers.targets, id: \.name) { target in
                        Text(target.label).tag(target.name)
                    }
                    if case .document(let name, _) = view.aim {
                        Text(name).tag(Self.documentTag)
                    }
                } label: {
                    ToolRow(title: "Fit toward", detail: "The response the bands aim for", symbol: "scope")
                }
                .disabled(view.fitting)
                Button { choosingTarget = true } label: {
                    ToolRow(title: "Choose a target file…", detail: "A response of your own to aim for", symbol: "doc.text")
                }
                .disabled(view.fitting)
                .fileImporter(isPresented: $choosingTarget, allowedContentTypes: Self.fileTypes) { result in
                    switch result {
                    case .success(let url): measured.aimAtDocument(url)
                    case .failure(let failure): library.report(failure)
                    }
                }
            } header: {
                Text("Target")
            }
            Section {
                Button { choosingMeasurement = true } label: {
                    ToolRow(title: "Fit a measurement…", detail: "AutoEq's CSV or REW's exported text", symbol: "waveform.path.ecg")
                }
                .disabled(view.fitting || view.aim == nil)
                .fileImporter(isPresented: $choosingMeasurement, allowedContentTypes: Self.fileTypes) { result in
                    switch result {
                    case .success(let url): measured.fit(url)
                    case .failure(let failure): library.report(failure)
                    }
                }
            } header: {
                Text("A measurement file")
            } footer: {
                if let said = view.said { Text(said) }
            }
            Section {
                if view.sounding {
                    HStack(spacing: 12) {
                        ProgressView().accessibilityHidden(true)
                        Text("Measuring…")
                        Spacer(minLength: 0)
                        Button("Stop", role: .cancel) { measured.stopMeasuring() }
                            .buttonStyle(.bordered)
                            .buttonBorderShape(.capsule)
                    }
                } else {
                    Button { measured.measure() } label: {
                        ToolRow(title: view.fitting ? "Fitting…" : "Measure", detail: "Pauses the music and plays a sweep through the speaker", symbol: "mic")
                    }
                    .disabled(view.fitting || view.aim == nil)
                }
                if view.denied {
                    Button {
                        if let settings = URL(string: UIApplication.openSettingsURLString) { openURL(settings) }
                    } label: {
                        ToolRow(title: "Allow the microphone", detail: "Resonate needs it to hear the sweep. Open Settings", symbol: "mic.slash")
                    }
                }
            } header: {
                Text("The microphone")
            } footer: {
                Text("Hold the iPhone where your ears would be and keep the room quiet while the sweep plays.")
            }
            Section {
                if let calibration = view.calibration {
                    Text(calibration.name)
                    Button("Forget the calibration", systemImage: "xmark.circle") { measured.forgetCalibration() }
                        .disabled(view.fitting)
                } else {
                    Text("None, so the microphone is taken flat")
                        .foregroundStyle(.secondary)
                    Button("Choose a calibration…", systemImage: "doc.text") { choosingCalibration = true }
                        .disabled(view.fitting)
                        .fileImporter(isPresented: $choosingCalibration, allowedContentTypes: Self.fileTypes) { result in
                            switch result {
                            case .success(let url): measured.calibrate(url)
                            case .failure(let failure): library.report(failure)
                            }
                        }
                }
            } header: {
                Text("Microphone calibration")
            }
        }
        .navigationTitle("Measure with a microphone")
        .onDisappear { measured.stopMeasuring() }
    }

    private static func tag(of aim: MeasurementAim?) -> String {
        switch aim {
        case .builtIn(let name): name
        case .document: documentTag
        case nil: ""
        }
    }

    private static func choose(_ tag: String, _ measured: MeasuredCorrectionState) {
        guard tag != documentTag else { return }
        measured.aim(at: .builtIn(name: tag))
    }
}
