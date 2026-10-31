import ResonateLogic
import ResonateNative
import SwiftUI

struct ZonesPresentation: ViewModifier {
    @Environment(Navigation.self) private var navigation
    let presenting: Bool

    func body(content: Content) -> some View {
        content.sheet(isPresented: Binding(
            get: { presenting && navigation.zones },
            set: { navigation.zones = $0 }
        )) {
            ZonesSheet().presentationDetents([.medium, .large])
        }
    }
}

struct ZonesSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    @State private var renaming: PlayerZone?
    @State private var name = ""
    @State private var removing: PlayerZone?
    @State private var choosing: PlayerZone?
    @State private var addressing = false

    var body: some View {
        let shown = library.zones.first(where: \.shown)?.id
        NavigationStack {
            List {
                Section {
                    ForEach(library.zones, id: \.id) { zone in
                        Button {
                            library.showZone(zone)
                        } label: {
                            HStack {
                                Image(systemName: zone.shown ? "checkmark.circle.fill" : "circle")
                                    .foregroundStyle(zone.shown ? Color.worn : Color.secondary)
                                VStack(alignment: .leading) {
                                    Text(zone.name)
                                    Text(ZonePlan.line(zone.transport, playing: zone.playing, on: zone.rendererName))
                                        .font(.subheadline)
                                        .foregroundStyle(.secondary)
                                }
                            }
                        }
                        .accessibilityAddTraits(zone.shown ? .isSelected : [])
                        .contextMenu { actions(for: zone, shown: shown) }
                        .swipeActions {
                            if ZonePlan.mayRemove(zone.id) {
                                Button("Remove", role: .destructive) { removing = zone }
                                    .tint(.red)
                            }
                        }
                    }
                } footer: {
                    Text("Each zone plays a queue of its own. iOS sends every zone to the one route it chose, so zones here share that output unless one plays on a renderer.")
                }
                Section {
                    Button("Add a zone", systemImage: "plus") { library.addZone() }
                }
                Section {
                    Toggle(isOn: Binding(get: { library.lookingForRenderers }, set: { library.lookForRenderers($0) })) {
                        Text("Find renderers")
                        Text(library.lookingForRenderers ? "\(library.renderers.count) found on this network" : "Streamers and amplifiers on this network, as outputs for a zone")
                    }
                    Button("Add a renderer by address", systemImage: "plus") { addressing = true }
                } header: {
                    Text("Network renderers")
                } footer: {
                    Text(library.rendererAddresses.isEmpty
                        ? "A network that hides its renderers from a search, or an iPhone signed for free, reaches one by the address of its description."
                        : "\(library.rendererAddresses.count) added by address.")
                }
            }
            .sheet(isPresented: $addressing) { RendererAddressSheet() }
            .task(id: library.lookingForRenderers || !library.rendererAddresses.isEmpty) {
                while library.lookingForRenderers || !library.rendererAddresses.isEmpty, !Task.isCancelled {
                    library.readRenderers()
                    try? await Task.sleep(for: .seconds(2))
                }
            }
            .sheet(isPresented: Binding(
                get: { choosing != nil },
                set: { if !$0 { choosing = nil } }
            )) {
                if let zone = choosing { RendererPicker(zone: zone) }
            }
            .navigationTitle("Zones")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) { dismiss() }
                }
            }
            .alert("Rename zone", isPresented: Binding(
                get: { renaming != nil },
                set: { if !$0 { renaming = nil } }
            )) {
                TextField("Zone name", text: $name)
                Button("Save") {
                    if let zone = renaming { library.renameZone(zone, to: name) }
                    renaming = nil
                }
                .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
                Button("Cancel", role: .cancel) { renaming = nil }
            }
            .confirmationDialog(
                "Remove \(removing?.name ?? "")?",
                isPresented: Binding(
                    get: { removing != nil },
                    set: { if !$0 { removing = nil } }
                ),
                titleVisibility: .visible
            ) {
                Button("Remove", role: .destructive) {
                    if let zone = removing { library.removeZone(zone) }
                    removing = nil
                }
            } message: {
                Text("Its queue goes with it and it stops playing.")
            }
        }
    }

    @ViewBuilder
    private func actions(for zone: PlayerZone, shown: UInt32?) -> some View {
        if let shown, ZonePlan.mayHandOver(to: zone.id, shown: shown, shownHolds: !library.queue.isEmpty) {
            Button("Hand the song over here", systemImage: "arrow.right.circle") { library.handOver(to: zone) }
        }
        Button("Play on a renderer", systemImage: "hifispeaker") { choosing = zone }
        if zone.renderer != nil {
            Button("Play on this iPhone", systemImage: "iphone") { library.playZone(zone, on: nil) }
        }
        Button("Rename", systemImage: "pencil") {
            name = zone.name
            renaming = zone
        }
        if ZonePlan.mayRemove(zone.id) {
            Button("Remove", systemImage: "trash", role: .destructive) { removing = zone }
        }
    }
}

struct RendererPicker: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    let zone: PlayerZone

    var body: some View {
        let playingOn = library.zones.first { $0.id == zone.id }?.renderer
        NavigationStack {
            List {
                if !library.lookingForRenderers, library.rendererAddresses.isEmpty, library.renderers.isEmpty {
                    Text("Turn on Find renderers, or add one by its address, to reach renderers.")
                        .foregroundStyle(.secondary)
                } else if library.renderers.isEmpty {
                    Text("No renderer has answered yet.")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(library.renderers, id: \.node) { renderer in
                        Button {
                            library.playZone(zone, on: renderer)
                            dismiss()
                        } label: {
                            HStack {
                                Image(systemName: renderer.node == playingOn ? "checkmark.circle.fill" : "circle")
                                    .foregroundStyle(renderer.node == playingOn ? Color.worn : Color.secondary)
                                VStack(alignment: .leading) {
                                    Text(renderer.name)
                                    Text(renderer.host)
                                        .font(.subheadline)
                                        .foregroundStyle(.secondary)
                                }
                            }
                        }
                        .accessibilityAddTraits(renderer.node == playingOn ? .isSelected : [])
                    }
                }
            }
            .navigationTitle("Play \(zone.name) on")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) { dismiss() }
                }
            }
        }
        .presentationDetents([.medium, .large])
    }
}

struct RendererAddressSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    @State private var typed = ""
    @FocusState private var focused: Bool

    var body: some View {
        NavigationStack {
            List {
                Section {
                    TextField("http://192.168.1.20:49152/description.xml", text: $typed)
                        .keyboardType(.URL)
                        .textContentType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .focused($focused)
                        .submitLabel(.done)
                        .onSubmit(add)
                    Button("Add", action: add)
                        .disabled(typed.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                } header: {
                    Text("Description address")
                }
                if !library.rendererAddresses.isEmpty {
                    Section {
                        ForEach(library.rendererAddresses, id: \.self) { address in
                            Text(address)
                                .swipeActions {
                                    Button("Remove", role: .destructive) { library.forgetRendererAddress(address) }
                                }
                        }
                    } header: {
                        Text("Added")
                    }
                }
            }
            .navigationTitle("Add a renderer by address")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) { dismiss() }
                }
            }
            .onAppear { focused = true }
        }
        .presentationDetents([.medium, .large])
    }

    private func add() {
        let chosen = typed.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !chosen.isEmpty else { return }
        library.addRendererAddress(chosen)
        typed = ""
    }
}
