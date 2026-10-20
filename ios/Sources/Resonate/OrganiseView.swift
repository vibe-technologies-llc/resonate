import ResonateLogic
import ResonateNative
import SwiftUI

struct OrganiseSheet: View {
    private static let layoutKey = "organise-layout"
    private static let listedAtMost = 50
    private static let settings = UserDefaults(suiteName: "library-features") ?? .standard

    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    let folder: MusicFolder
    @State private var layout = OrganiseSheet.settings.string(forKey: OrganiseSheet.layoutKey) ?? defaultOrganiseLayout()
    @State private var plan: OrganisePlan?
    @State private var plannedFor: String?
    @State private var failedFor: String?
    @State private var planning = false
    @State private var confirming = false

    var body: some View {
        let problem = organiseLayoutProblem(layout: layout)
        let current = plannedFor == layout ? plan : nil
        let waiting = problem == nil && current == nil && failedFor != layout
        NavigationStack {
            Form {
                Section {
                    TextField("Layout", text: $layout)
                        .font(.body.monospaced())
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    if layout != defaultOrganiseLayout() {
                        Button("Use the usual layout") { layout = defaultOrganiseLayout() }
                    }
                } header: {
                    Text("Layout")
                } footer: {
                    if let problem {
                        Text(problem).foregroundStyle(.red)
                    } else {
                        Text("Fields: {albumartist} {artist} {album} {title} {year} {disc} {track} {ext}. Slashes make folders; {disc} is written only for albums of more than one disc.")
                    }
                }
                if planning || waiting {
                    Section { ProgressView().frame(maxWidth: .infinity) }
                } else if let plan = current {
                    Section {
                        LabeledContent("To move", value: counted(plan.moves.count, "song"))
                        LabeledContent("Already in place", value: counted(Int(plan.unchanged), "song"))
                        if plan.taken > 0 {
                            LabeledContent("Place taken", value: counted(Int(plan.taken), "song"))
                        }
                        if plan.unplaced > 0 {
                            LabeledContent("Not enough tags", value: counted(Int(plan.unplaced), "song"))
                        }
                    } header: {
                        Text("Preview")
                    } footer: {
                        Text("A song whose place another file holds, or that names nothing to file it by, stays where it is.")
                    }
                    if !plan.moves.isEmpty {
                        Section {
                            ForEach(plan.moves.prefix(Self.listedAtMost), id: \.id) { move in
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(move.to.joined(separator: " / ")).font(.subheadline)
                                    Text(shown(move.from)).font(.caption).foregroundStyle(.secondary)
                                }
                                .lineLimit(2)
                            }
                            if plan.moves.count > Self.listedAtMost {
                                Text("and \(counted(plan.moves.count - Self.listedAtMost, "more song"))").foregroundStyle(.secondary)
                            }
                        } header: {
                            Text("Moves")
                        }
                    }
                }
            }
            .navigationTitle("Organise \(folder.name)")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Organise") { confirming = true }
                        .disabled(problem != nil || planning || (current?.moves.isEmpty ?? true) || library.features.organising)
                }
            }
            .confirmationDialog(
                "Move \(counted(current?.moves.count ?? 0, "song"))?",
                isPresented: $confirming,
                titleVisibility: .visible
            ) {
                Button("Organise") {
                    guard let plan = current else { return }
                    Self.settings.set(layout, forKey: Self.layoutKey)
                    library.features.organise(folder, plan.moves)
                    dismiss()
                }
            } message: {
                Text("The files move inside \(folder.name), each with its lyrics beside it. Favourites, plays and playlists follow them, and Library tools can put them back.")
            }
        }
        .task(id: layout) { await preview(problem == nil) }
    }

    private func preview(_ readable: Bool) async {
        guard readable else {
            plan = nil
            plannedFor = nil
            return
        }
        try? await Task.sleep(for: .milliseconds(300))
        guard !Task.isCancelled else { return }
        let asked = layout
        planning = true
        defer { planning = false }
        do {
            let found = try await library.features.organisePlan(folder, layout: asked)
            guard !Task.isCancelled else { return }
            plan = found
            plannedFor = asked
            failedFor = nil
        } catch {
            if !Task.isCancelled { failedFor = asked }
            library.report(error)
        }
    }

    private func shown(_ uri: String) -> String {
        let prefix = library.folders.prefix(of: folder.id) + "/"
        let relative = uri.hasPrefix(prefix) ? String(uri.dropFirst(prefix.count)) : uri
        return "from " + relative.split(separator: "/").map { $0.removingPercentEncoding ?? String($0) }.joined(separator: " / ")
    }
}
