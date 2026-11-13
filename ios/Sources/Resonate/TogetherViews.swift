import CoreImage.CIFilterBuiltins
import ResonateLogic
import ResonateNative
import SwiftUI
import UIKit

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
        .accessibilityLabel("together.link.qr-code.label")
        .task(id: text) {
            let asked = text
            let image = try? await Work.aside { QRCodes.drawn(asked) }
            guard !Task.isCancelled else { return }
            drawn = image
        }
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
        Button(said(copies > 0 ? "together.link.copied" : "common.copy"), systemImage: "doc.on.doc") {
            UIPasteboard.general.string = link
            copies += 1
        }
        .announced(copies > 0 ? said("together.link.copied") : nil)
        ShareLink(item: link) {
            Label("common.share", systemImage: "square.and.arrow.up")
        }
        Button(said(showingCode ? "together.link.hide-qr-code" : "together.link.qr-code"), systemImage: "qrcode") {
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
                        Button("together.open-sync-settings", systemImage: "arrow.triangle.2.circlepath") {
                            dismiss()
                            navigation.tab = .settings
                            navigation.paths[.settings] = [.sync]
                        }
                    }
                } else {
                    Section {
                        Text("together.share.explained")
                        Button("together.share", systemImage: "person.2") {
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
                Button(presses.asks(.reset) ? TogetherWords.pressAgain : said("together.links.renew"), systemImage: "arrow.clockwise") {
                    press(.reset) { await library.features.together.reset(id, removing: nil) }
                }
                .disabled(asking)
            }
            Button(presses.asks(.leave) ? TogetherWords.pressAgain : said("together.leave"), systemImage: "rectangle.portrait.and.arrow.right", role: .destructive) {
                press(.leave) {
                    let problem = await library.features.together.leaveList(id)
                    if problem == nil { dismiss() }
                    return problem
                }
            }
            .disabled(asking)
        } footer: {
            if list.mayEdit {
                Text("together.links.renew.footer")
            }
        }
    }

    private func peopleSection(_ list: SharedPlaylist) -> some View {
        Section("together.people") {
            ForEach(list.people, id: \.id) { person in
                HStack {
                    Text(TogetherWords.person(person)).titleLines()
                    Spacer()
                    if list.mayEdit, !person.isMe {
                        let removing = SharedAct.remove(person.id)
                        Button(presses.asks(removing) ? TogetherWords.pressAgain : said("common.remove"), role: .destructive) {
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
            Section("together.not-held") {
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
    @State private var handingTo: TogetherPerson?

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
                        Button("common.close", systemImage: "xmark") { together.leaveParty() }
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
        .navigationTitle("party.title")
        .confirmationDialog(
            handingTo.map(PartyWords.handOverAsked) ?? "",
            isPresented: Binding(get: { handingTo != nil }, set: { if !$0 { handingTo = nil } }),
            titleVisibility: .visible
        ) {
            if let person = handingTo {
                Button("party.hand-over") { together.handOver(to: person) }
            }
            Button("common.cancel", role: .cancel) {}
        }
        .task(id: phase == .active) {
            guard phase == .active else { return }
            await together.followParty()
        }
    }

    @ViewBuilder
    private func beginSections(_ together: TogetherState) -> some View {
        Section {
            Text("party.explained")
            if let gate = together.partyGate {
                Text(gate).foregroundStyle(.secondary).announced(gate)
            } else {
                Button("party.begin", systemImage: "person.2.wave.2") { together.beginParty() }
                    .disabled(together.joining)
                Button("party.paste-link", systemImage: "doc.on.clipboard") {
                    together.accept(UIPasteboard.general.string ?? "")
                }
                .disabled(together.joining)
            }
        }
        if together.joining {
            Section {
                ProgressView("party.reaching")
            }
        }
    }

    @ViewBuilder
    private func hostSections(_ party: ListeningParty, _ together: TogetherState) -> some View {
        InvitationLinkSection(title: said("party.link"), link: party.link)
        peopleSection(party, together)
        PartyChatSection(party: party, together: together)
        if !party.requests.isEmpty {
            Section("party.requests") {
                ForEach(party.requests, id: \.id) { request in
                    VStack(alignment: .leading, spacing: 8) {
                        PartySongLines(song: request)
                        HStack(spacing: 12) {
                            Button("party.requests.play-next") { together.playNext(request) }
                            Button("party.requests.pass") { together.passOver(request) }
                        }
                        .buttonStyle(.bordered)
                    }
                }
            }
        }
        Section {
            Button("party.end", systemImage: "stop.circle", role: .destructive) { together.leaveParty() }
        }
    }

    @ViewBuilder
    private func guestSections(_ party: ListeningParty, _ together: TogetherState) -> some View {
        let following = PartyWords.following(party.following)
        Section("party.now") {
            if let song = party.song {
                PartySongLines(song: song)
            }
            Text(following).announced(following)
        }
        peopleSection(party, together)
        PartyChatSection(party: party, together: together)
        Section {
            Button("party.leave", systemImage: "rectangle.portrait.and.arrow.right", role: .destructive) { together.leaveParty() }
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
                        Button("party.hand-over") { handingTo = person }
                            .buttonStyle(.bordered)
                        Button("common.remove", role: .destructive) { together.turnOut(person) }
                            .buttonStyle(.bordered)
                    }
                }
            }
            if let handing = party.handing {
                let words = PartyWords.handing(handing)
                Text(words).foregroundStyle(.secondary).announced(words)
            }
        } header: {
            Text("together.people")
        } footer: {
            PartyRoundTrip(together: together)
        }
    }
}

private struct PartyChatSection: View {
    let party: ListeningParty
    let together: TogetherState
    @State private var typed = ""

    var body: some View {
        Section("party.chat") {
            if party.chat.isEmpty {
                Text("party.chat.nothing-yet").foregroundStyle(.secondary)
            }
            ForEach(party.chat.suffix(PartyWords.chatLinesShown), id: \.id) { line in
                VStack(alignment: .leading, spacing: 2) {
                    Text(PartyWords.saidBy(line)).font(.caption.weight(.medium)).foregroundStyle(.secondary).titleLines()
                    Text(PartyWords.shown(line))
                }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(PartyWords.heard(line))
            }
            HStack(spacing: 4) {
                ForEach(PartyWords.reactions, id: \.self) { reaction in
                    Button(PartyWords.reactionDrawn(reaction)) { together.react(reaction) }
                        .buttonStyle(.borderless)
                        .font(.title3)
                        .frame(maxWidth: .infinity)
                        .accessibilityLabel(PartyWords.reactWith(reaction))
                }
            }
            HStack {
                TextField("party.chat.say-something", text: $typed)
                    .submitLabel(.send)
                    .onSubmit(send)
                Button("party.chat.send", systemImage: "paperplane.fill", action: send)
                    .labelStyle(.iconOnly)
                    .disabled(typed.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
    }

    private func send() {
        guard !typed.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        together.say(typed)
        typed = ""
    }
}

private struct PartyRoundTrip: View {
    let together: TogetherState

    var body: some View {
        if let said = PartyWords.roundTrip(together.partyRoundTripMs) {
            Text(said)
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
