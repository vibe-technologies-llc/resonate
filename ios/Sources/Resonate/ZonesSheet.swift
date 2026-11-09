import ResonateLogic
import ResonateNative
import SwiftUI

struct ZonesPresentation: ViewModifier {
    @Environment(Navigation.self) private var navigation
    let insidePlayer: Bool

    func body(content: Content) -> some View {
        content.sheet(isPresented: Binding(
            get: { navigation.nowPlaying == insidePlayer && navigation.zones },
            set: { navigation.zones = $0 }
        )) {
            ZonesSheet().sheetSized().presentationDetents([.medium, .large])
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
    @State private var addressingReceiver = false

    private var reaching: Bool {
        library.lookingForRenderers || !library.rendererAddresses.isEmpty || !library.receiverAddresses.isEmpty
    }

    private var addressesFooter: String {
        let added = library.rendererAddresses.count + library.receiverAddresses.count
        if added == 0 {
            return said("zones.addresses.none")
        }
        return said("zones.addresses.added", added)
    }

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
                                    .accessibilityHidden(true)
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
                                Button("common.remove", role: .destructive) { removing = zone }
                                    .tint(.red)
                            }
                        }
                    }
                } footer: {
                    Text("zones.footer")
                }
                Section {
                    Button("zones.add", systemImage: "plus") { library.addZone() }
                }
                Section {
                    Toggle(isOn: Binding(get: { library.lookingForRenderers }, set: { library.lookForRenderers($0) })) {
                        Text("zones.renderers.find")
                        Text(library.lookingForRenderers ? said("zones.renderers.found", library.renderers.count) : said("zones.renderers.find.detail"))
                    }
                    Button("zones.renderers.add-by-address", systemImage: "plus") { addressing = true }
                    Button("zones.receivers.add-by-address", systemImage: "plus") { addressingReceiver = true }
                } header: {
                    Text("zones.renderers")
                } footer: {
                    Text(addressesFooter)
                }
            }
            .sheet(isPresented: $addressing) { RendererAddressSheet(kind: .renderer).sheetSized() }
            .sheet(isPresented: $addressingReceiver) { RendererAddressSheet(kind: .receiver).sheetSized() }
            .task(id: reaching) {
                while reaching, !Task.isCancelled {
                    library.readRenderers()
                    try? await Task.sleep(for: .seconds(2))
                }
            }
            .sheet(isPresented: Binding(
                get: { choosing != nil },
                set: { if !$0 { choosing = nil } }
            )) {
                if let zone = choosing { RendererPicker(zone: zone).sheetSized() }
            }
            .navigationTitle("zones.title")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) { dismiss() }
                }
            }
            .alert("zones.rename.title", isPresented: Binding(
                get: { renaming != nil },
                set: { if !$0 { renaming = nil } }
            )) {
                TextField("zones.rename.field", text: $name)
                Button("common.save") {
                    if let zone = renaming { library.renameZone(zone, to: name) }
                    renaming = nil
                }
                .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
                Button("common.cancel", role: .cancel) { renaming = nil }
            }
            .confirmationDialog(
                said("zones.remove.confirm", removing?.name ?? ""),
                isPresented: Binding(
                    get: { removing != nil },
                    set: { if !$0 { removing = nil } }
                ),
                titleVisibility: .visible
            ) {
                Button("common.remove", role: .destructive) {
                    if let zone = removing { library.removeZone(zone) }
                    removing = nil
                }
            } message: {
                Text("zones.remove.confirm.message")
            }
        }
    }

    @ViewBuilder
    private func actions(for zone: PlayerZone, shown: UInt32?) -> some View {
        if let shown, ZonePlan.mayHandOver(to: zone.id, shown: shown, shownHolds: !library.queue.isEmpty) {
            Button("zones.hand-over", systemImage: "arrow.right.circle") { library.handOver(to: zone) }
        }
        Button("zones.play-on-renderer", systemImage: "hifispeaker") { choosing = zone }
        if zone.renderer != nil {
            Button("zones.play-on-iphone", systemImage: "iphone") { library.playZone(zone, on: nil) }
        }
        Button("common.rename", systemImage: "pencil") {
            name = zone.name
            renaming = zone
        }
        if ZonePlan.mayRemove(zone.id) {
            Button("common.remove", systemImage: "trash", role: .destructive) { removing = zone }
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
                if !library.lookingForRenderers, library.rendererAddresses.isEmpty, library.receiverAddresses.isEmpty, library.renderers.isEmpty {
                    Text("zones.renderers.off")
                        .foregroundStyle(.secondary)
                } else if library.renderers.isEmpty {
                    Text("zones.renderers.none")
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
                                    .accessibilityHidden(true)
                                VStack(alignment: .leading) {
                                    Text(renderer.name)
                                    Text(ZonePlan.rendererLine(host: renderer.host, viaCast: renderer.kind == .cast))
                                        .font(.subheadline)
                                        .foregroundStyle(.secondary)
                                }
                            }
                        }
                        .accessibilityAddTraits(renderer.node == playingOn ? .isSelected : [])
                    }
                }
            }
            .navigationTitle(said("zones.renderer-picker.title", zone.name))
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

enum AddressKind {
    case renderer
    case receiver

    var title: String {
        switch self {
        case .renderer: said("zones.renderers.add-by-address")
        case .receiver: said("zones.receivers.add-by-address")
        }
    }

    var field: String {
        switch self {
        case .renderer: "http://192.168.1.20:49152/description.xml"
        case .receiver: said("zones.receivers.address-prompt")
        }
    }

    var header: String {
        switch self {
        case .renderer: said("zones.renderers.address")
        case .receiver: said("zones.receivers.address")
        }
    }
}

struct RendererAddressSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    @State private var typed = ""
    @FocusState private var focused: Bool
    let kind: AddressKind

    private var held: [String] {
        switch kind {
        case .renderer: library.rendererAddresses
        case .receiver: library.receiverAddresses
        }
    }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    TextField(kind.field, text: $typed)
                        .keyboardType(.URL)
                        .textContentType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .focused($focused)
                        .submitLabel(.done)
                        .onSubmit(add)
                    Button("common.add", action: add)
                        .disabled(typed.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                } header: {
                    Text(kind.header)
                }
                if !held.isEmpty {
                    Section {
                        ForEach(held, id: \.self) { address in
                            Text(address)
                                .swipeActions {
                                    Button("common.remove", role: .destructive) { forget(address) }
                                }
                        }
                    } header: {
                        Text("zones.addresses.added-header")
                    }
                }
            }
            .navigationTitle(kind.title)
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
        switch kind {
        case .renderer: library.addRendererAddress(chosen)
        case .receiver: library.addReceiverAddress(chosen)
        }
        typed = ""
    }

    private func forget(_ address: String) {
        switch kind {
        case .renderer: library.forgetRendererAddress(address)
        case .receiver: library.forgetReceiverAddress(address)
        }
    }
}
