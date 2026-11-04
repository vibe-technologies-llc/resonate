import CoreImage.CIFilterBuiltins
import ResonateLogic
import ResonateNative
import SwiftUI
import UIKit

@MainActor
enum QRCodes {
    private static let context = CIContext()
    private static let correction = "M"

    static func drawn(_ text: String) -> CGImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(text.utf8)
        filter.correctionLevel = correction
        guard let output = filter.outputImage else { return nil }
        return context.createCGImage(output, from: output.extent)
    }
}

struct QRCodeImage: View {
    private static let quietZone: CGFloat = 12
    private static let corner: CGFloat = 12

    @ScaledMetric(relativeTo: .body) private var edge: CGFloat = 220
    @State private var drawn: CGImage?
    let text: String

    var body: some View {
        Group {
            if let drawn {
                Image(decorative: drawn, scale: 1)
                    .interpolation(.none)
                    .resizable()
                    .scaledToFit()
            } else {
                Color.white
            }
        }
        .padding(Self.quietZone)
        .frame(width: edge, height: edge)
        .background(Color.white, in: .rect(cornerRadius: Self.corner))
        .accessibilityElement()
        .accessibilityLabel("QR code of this link")
        .task(id: text) { drawn = QRCodes.drawn(text) }
    }
}

struct InvitationLinkSection: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var copies = 0
    @State private var showingCode = false
    let title: String
    let link: String

    var body: some View {
        Section(title) {
            VStack(alignment: .leading, spacing: 12) {
                Text(link)
                    .font(.footnote.monospaced())
                    .lineLimit(3)
                    .truncationMode(.middle)
                    .textSelection(.enabled)
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 12) { actions }
                    VStack(alignment: .leading, spacing: 8) { actions }
                }
                .buttonStyle(.bordered)
                if showingCode {
                    QRCodeImage(text: link)
                        .frame(maxWidth: .infinity)
                }
            }
            .padding(.vertical, 4)
        }
        .sensoryFeedback(.success, trigger: copies)
    }

    @ViewBuilder
    private var actions: some View {
        Button(copies > 0 ? "Copied" : "Copy", systemImage: "doc.on.doc") {
            UIPasteboard.general.string = link
            copies += 1
        }
        .announced(copies > 0 ? "Copied" : nil)
        ShareLink(item: link) {
            Label("Share", systemImage: "square.and.arrow.up")
        }
        Button(showingCode ? "Hide QR" : "QR code", systemImage: "qrcode") {
            withAnimation(reduceMotion ? nil : Arrival.animation) { showingCode.toggle() }
        }
    }
}

struct SharedPlaylistSheet: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(\.dismiss) private var dismiss
    @State private var presses = SecondPress<SharedAct>()
    @State private var failure: String?
    @State private var asking = false
    let id: UInt64
    let name: String

    var body: some View {
        let together = library.features.together
        let list = together.shared(id)
        NavigationStack {
            ThemedForm {
                if let failure {
                    Section {
                        Text(failure).foregroundStyle(Color.failure).announced(failure)
                    }
                }
                if let list {
                    sharedSections(list)
                } else if let gate = together.sharingGate {
                    Section {
                        Text(gate).announced(gate)
                        Button("Open sync settings", systemImage: "arrow.triangle.2.circlepath") {
                            dismiss()
                            navigation.tab = .settings
                            navigation.paths[.settings] = [.sync]
                        }
                    }
                } else {
                    Section {
                        Text("Other people can follow this playlist or edit it with you. They get a link, and the songs stay on each person's own device.")
                        Button("Share this playlist", systemImage: "person.2") {
                            run { await together.share(id) }
                        }
                        .disabled(asking)
                    }
                }
            }
            .navigationTitle(name)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(role: .close) { dismiss() } }
            }
            .task(id: list == nil) { await together.watch(id) }
        }
    }

    @ViewBuilder
    private func sharedSections(_ list: SharedPlaylist) -> some View {
        if let reach = SharedPlaylistWords.reach(list.reach) {
            Section {
                Text(reach).announced(reach)
            }
        }
        if !list.mayEdit {
            Section {
                Text(SharedPlaylistWords.following)
            }
        }
        if let edit = list.editLink {
            InvitationLinkSection(title: SharedPlaylistWords.editLink, link: edit)
        }
        InvitationLinkSection(title: SharedPlaylistWords.followLink, link: list.followLink)
        peopleSection(list)
        notHeldSection(list.notHeld)
        Section {
            if list.mayEdit {
                Button(presses.asks(.reset) ? TogetherWords.pressAgain : "Make new links", systemImage: "arrow.clockwise") {
                    press(.reset) { await library.features.together.reset(id, removing: nil) }
                }
                .disabled(asking)
            }
            Button(presses.asks(.leave) ? TogetherWords.pressAgain : "Leave", systemImage: "rectangle.portrait.and.arrow.right", role: .destructive) {
                press(.leave) {
                    let problem = await library.features.together.leaveList(id)
                    if problem == nil { dismiss() }
                    return problem
                }
            }
            .disabled(asking)
        } footer: {
            if list.mayEdit {
                Text("New links stop the old ones working. Everyone still here moves across by themselves.")
            }
        }
    }

    private func peopleSection(_ list: SharedPlaylist) -> some View {
        Section("People") {
            ForEach(list.people, id: \.id) { person in
                HStack {
                    Text(TogetherWords.person(person)).titleLines()
                    Spacer()
                    if list.mayEdit, !person.isMe {
                        let removing = SharedAct.remove(person.id)
                        Button(presses.asks(removing) ? TogetherWords.pressAgain : "Remove", role: .destructive) {
                            press(removing) { await library.features.together.reset(id, removing: person.id) }
                        }
                        .buttonStyle(.bordered)
                        .disabled(asking)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func notHeldSection(_ titles: [String]) -> some View {
        if !titles.isEmpty {
            let lines = SharedPlaylistWords.notHeld(titles)
            Section("Songs you don't have") {
                ForEach(Array(lines.named.enumerated()), id: \.offset) { _, title in
                    Text(title).titleLines()
                }
                if let more = lines.more {
                    Text(more).foregroundStyle(.secondary)
                }
            }
        }
    }

    private func press(_ act: SharedAct, _ action: @escaping @MainActor () async -> String?) {
        guard presses.press(act) else { return }
        run(action)
    }

    private func run(_ action: @escaping @MainActor () async -> String?) {
        asking = true
        Task {
            failure = await action()
            asking = false
        }
    }
}

struct PartyView: View {
    @Environment(Library.self) private var library
    @Environment(\.scenePhase) private var phase

    var body: some View {
        let together = library.features.together
        ThemedForm {
            if let party = together.party {
                if let said = PartyWords.connection(party.connection) {
                    Section {
                        Text(said).announced(said)
                    }
                }
                if PartyWords.isOver(party.connection) {
                    Section {
                        Button("Close", systemImage: "xmark") { together.leaveParty() }
                    }
                } else if party.hosting {
                    hostSections(party, together)
                } else {
                    guestSections(party, together)
                }
            } else {
                beginSections(together)
            }
        }
        .navigationTitle("Listening party")
        .task(id: phase == .active) {
            guard phase == .active else { return }
            await together.followParty()
        }
    }

    @ViewBuilder
    private func beginSections(_ together: TogetherState) -> some View {
        Section {
            Text("Listen with other people, in step. They join by a link and hear what you play, each from their own copy.")
            if let gate = together.partyGate {
                Text(gate).foregroundStyle(.secondary).announced(gate)
            } else {
                Button("Begin a party", systemImage: "person.2.wave.2") { together.beginParty() }
                    .disabled(together.joining)
                Button("Paste a link", systemImage: "doc.on.clipboard") {
                    together.accept(UIPasteboard.general.string ?? "")
                }
                .disabled(together.joining)
            }
        }
        if together.joining {
            Section {
                ProgressView("Reaching the server…")
            }
        }
    }

    @ViewBuilder
    private func hostSections(_ party: ListeningParty, _ together: TogetherState) -> some View {
        InvitationLinkSection(title: "People join by this link", link: party.link)
        peopleSection(party, together)
        if !party.requests.isEmpty {
            Section("Asked for") {
                ForEach(Array(party.requests.enumerated()), id: \.offset) { at, request in
                    VStack(alignment: .leading, spacing: 8) {
                        PartySongLines(song: request)
                        HStack(spacing: 12) {
                            Button("Play next") { together.playNext(at) }
                            Button("Pass") { together.passOver(at) }
                        }
                        .buttonStyle(.bordered)
                    }
                }
            }
        }
        Section {
            Button("End the party", systemImage: "stop.circle", role: .destructive) { together.leaveParty() }
        }
    }

    @ViewBuilder
    private func guestSections(_ party: ListeningParty, _ together: TogetherState) -> some View {
        let following = PartyWords.following(party.following)
        Section("Now") {
            if let song = party.song {
                PartySongLines(song: song)
            }
            Text(following).announced(following)
        }
        peopleSection(party, together)
        Section {
            Button("Leave", systemImage: "rectangle.portrait.and.arrow.right", role: .destructive) { together.leaveParty() }
        } footer: {
            Text(PartyWords.guestAsks)
        }
    }

    private func peopleSection(_ party: ListeningParty, _ together: TogetherState) -> some View {
        Section {
            ForEach(party.people, id: \.id) { person in
                HStack {
                    Text(TogetherWords.person(person)).titleLines()
                    Spacer()
                    if party.hosting, !person.isMe {
                        Button("Remove", role: .destructive) { together.turnOut(person) }
                            .buttonStyle(.bordered)
                    }
                }
            }
        } header: {
            Text("People")
        } footer: {
            if let said = PartyWords.roundTrip(party.roundTripMs) {
                Text(said)
            }
        }
    }
}

struct PartySongLines: View {
    let song: PartyRequest

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(song.title).font(.body.weight(.medium)).titleLines()
            if !song.artist.isEmpty {
                Text(song.artist).font(.subheadline).foregroundStyle(.secondary).titleLines()
            }
        }
        .accessibilityElement(children: .combine)
    }
}
