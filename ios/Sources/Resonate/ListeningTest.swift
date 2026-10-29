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

    init(library: Library) {
        self.library = library
    }

    func begin(_ trials: UInt32, curve: EqualiserCurve) {
        guard test == nil, !beginning else { return }
        beginning = true
        Task {
            defer { beginning = false }
            do {
                let player = try await library.player()
                let made = try await Work.aside { try player.beginListeningTest(curve: curve, trials: trials) }
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
                ForEach(testing.lengths, id: \.self) { trials in
                    Button("Begin \(trials) trials", systemImage: "play.fill") { testing.begin(trials, curve: curve) }
                        .disabled(testing.beginning)
                }
            }
        } header: {
            Text("Listening test")
        } footer: {
            Text(ListeningTestWords.howItWorks)
        }
    }
}
