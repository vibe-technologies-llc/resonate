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
                                    Text(ZonePlan.line(zone.transport, playing: zone.playing))
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
                    Text("Each zone plays a queue of its own. iOS sends every zone to the one route it chose, so zones here share that output.")
                }
                Section {
                    Button("Add a zone", systemImage: "plus") { library.addZone() }
                }
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
        Button("Rename", systemImage: "pencil") {
            name = zone.name
            renaming = zone
        }
        if ZonePlan.mayRemove(zone.id) {
            Button("Remove", systemImage: "trash", role: .destructive) { removing = zone }
        }
    }
}
