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
            if view.fitting {
                ProgressView().frame(maxWidth: .infinity)
            }
        } header: {
            Text("Measured correction")
        } footer: {
            Text(view.said ?? "Fit a few bands to your own frequency-response measurement, as AutoEq's CSV or REW's exported text, toward a target.")
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
