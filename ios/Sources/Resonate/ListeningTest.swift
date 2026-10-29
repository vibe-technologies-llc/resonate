import Foundation
import Observation
import ResonateLogic
import ResonateNative
import SwiftUI

@MainActor
@Observable
final class ListeningTestState {
    @ObservationIgnored private unowned let library: Library

    private(set) var lengths = listeningTestLengths()
    private(set) var test: ListeningTest?
    private(set) var standing: TestStanding?
    private(set) var beginning = false
    private(set) var testing = ListeningTesting.equaliser
    private(set) var against: TestedSetting?
    private(set) var running = ListeningTesting.equaliser

    init(library: Library) {
        self.library = library
    }

    func choose(_ chosen: ListeningTesting, preferences: PlaybackPreferences?) {
        guard test == nil, !beginning else { return }
        testing = chosen
        against = chosen.kind.flatMap { kind in preferences.map { settingTestByDefault(kind: kind, preferences: $0) } }
    }

    func set(against setting: TestedSetting) {
        against = setting
    }

    func begin(_ trials: UInt32, curve: EqualiserCurve) {
        guard test == nil, !beginning else { return }
        let tested = testing
        let setting = tested == .equaliser ? nil : against
        if tested != .equaliser && setting == nil { return }
        beginning = true
        Task {
            defer { beginning = false }
            do {
                let player = try await library.player()
                let made = try await Work.aside { () throws -> ListeningTest in
                    if let setting { return try player.beginSettingTest(setting: setting, trials: trials) }
                    return try player.beginListeningTest(curve: curve, trials: trials)
                }
                running = tested
                test = made
                standing = made.standing()
            } catch {
                library.report(error)
            }
        }
    }

    func hear(_ hear: TestHear) {
        guard let test else { return }
        Task {
            do {
                let player = try await library.player()
                try await Work.aside { try player.hearInListeningTest(test: test, hear: hear) }
                standing = test.standing()
            } catch {
                library.report(error)
            }
        }
    }

    func say(_ arm: TestArm) {
        guard let test else { return }
        do {
            _ = try test.answer(said: arm)
            standing = test.standing()
        } catch {
            library.report(error)
        }
    }

    func end() {
        guard test != nil else { return }
        test = nil
        standing = nil
        library.request { try $0.endListeningTest() }
    }
}

struct ListeningTestSection: View {
    @Environment(Library.self) private var library

    let curve: EqualiserCurve

    var body: some View {
        let testing = library.features.listeningTest
        Section {
            if let standing = testing.standing {
                VStack(alignment: .leading, spacing: 4) {
                    Text(ListeningTestWords.standing(standing))
                    Text(ListeningTestWords.heard(standing)).font(.footnote).foregroundStyle(.secondary)
                    if !standing.verdicts.isEmpty {
                        Text(ListeningTestWords.verdicts(standing)).font(.footnote).foregroundStyle(.secondary)
                    }
                    if let last = ListeningTestWords.last(standing) {
                        Text(last).font(.footnote).foregroundStyle(.secondary)
                    }
                    if standing.finished {
                        Text(ListeningTestWords.conclusion(standing))
                    }
                }
                .accessibilityElement(children: .combine)
                if !standing.finished {
                    HStack {
                        Button("Hear A", systemImage: "play.fill") { testing.hear(.a) }
                        Button("Hear B", systemImage: "play.fill") { testing.hear(.b) }
                        Button("Hear X", systemImage: "play.fill") { testing.hear(.x) }
                    }
                    .buttonStyle(.bordered)
                    HStack {
                        Button("X is A", systemImage: "checkmark") { testing.say(.a) }
                        Button("X is B", systemImage: "checkmark") { testing.say(.b) }
                    }
                    .buttonStyle(.bordered)
                }
                Button(standing.finished ? "Done" : "End the test", systemImage: "stop.fill") { testing.end() }
            } else {
                Picker(
                    "Test",
                    selection: Binding(
                        get: { testing.testing },
                        set: { testing.choose($0, preferences: library.preferences) }
                    )
                ) {
                    ForEach(ListeningTesting.allCases, id: \.self) { Text($0.label).tag($0) }
                }
                .pickerStyle(.segmented)
                .disabled(testing.beginning)
                if testing.testing != .equaliser, let against = testing.against {
                    AgainstPickers(setting: against) { testing.set(against: $0) }
                }
                ForEach(testing.lengths, id: \.self) { trials in
                    Button(
                        testing.beginning ? ListeningTestWords.beginning(testing.testing) : "Begin \(trials) trials",
                        systemImage: "play.fill"
                    ) { testing.begin(trials, curve: curve) }
                        .disabled(testing.beginning)
                }
            }
        } header: {
            Text("Listening test")
        } footer: {
            Text((testing.standing == nil ? testing.testing : testing.running).howItWorks)
        }
    }
}

private struct AgainstPickers: View {
    let setting: TestedSetting
    let choose: (TestedSetting) -> Void

    var body: some View {
        Text(ListeningTestWords.playedAs(setting)).font(.subheadline)
        switch setting {
        case .restoration(let a, let b):
            Picker(
                "B is",
                selection: Binding(get: { b }, set: { choose(.restoration(a: a, b: $0)) })
            ) {
                ForEach(LossyRestoration.every.filter { $0 != a }, id: \.self) {
                    Text(ListeningTestWords.restorationLabel($0)).tag($0)
                }
            }
        case .resampling(let aQuality, let aPhase, let bQuality, let bPhase):
            Picker(
                "B's quality",
                selection: Binding(
                    get: { bQuality },
                    set: { choose(.resampling(aQuality: aQuality, aPhase: aPhase, bQuality: $0, bPhase: bPhase)) }
                )
            ) {
                ForEach(ResamplingQuality.every, id: \.self) {
                    Text(ListeningTestWords.qualityLabel($0)).tag($0)
                }
            }
            Picker(
                "B's phase",
                selection: Binding(
                    get: { bPhase },
                    set: { choose(.resampling(aQuality: aQuality, aPhase: aPhase, bQuality: bQuality, bPhase: $0)) }
                )
            ) {
                ForEach(ResamplingPhase.every, id: \.self) {
                    Text(ListeningTestWords.phaseLabel($0)).tag($0)
                }
            }
        case .dither(let a, let b):
            Picker(
                "B is",
                selection: Binding(get: { b }, set: { choose(.dither(a: a, b: $0)) })
            ) {
                ForEach(ListeningTestWords.ditherChoices.filter { $0 != a }, id: \.self) {
                    Text(ListeningTestWords.ditherLabel($0)).tag($0)
                }
            }
        }
    }
}
