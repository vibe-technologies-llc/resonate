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
                    TextField("organise.layout", text: $layout)
                        .font(.body.monospaced())
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    if layout != defaultOrganiseLayout() {
                        Button("organise.layout.usual") { layout = defaultOrganiseLayout() }
                    }
                } header: {
                    Text("organise.layout")
                } footer: {
                    if let problem {
                        Text(problem).foregroundStyle(Color.failure).announced(problem)
                    } else {
                        Text("organise.layout.fields")
                    }
                }
                if planning || waiting {
                    Section { ProgressView().accessibilityLabel("organise.planning").frame(maxWidth: .infinity) }
                } else if let plan = current {
                    Section {
                        LabeledContent("organise.to-move", value: said("counted.songs", plan.moves.count))
                        LabeledContent("organise.in-place", value: said("counted.songs", Int(plan.unchanged)))
                        if plan.taken > 0 {
                            LabeledContent("organise.taken", value: said("counted.songs", Int(plan.taken)))
                        }
                        if plan.unplaced > 0 {
                            LabeledContent("organise.unplaced", value: said("counted.songs", Int(plan.unplaced)))
                        }
                    } header: {
                        Text("organise.preview")
                    } footer: {
                        Text("organise.preview.footer")
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
                                Text(said("organise.moves.more", plan.moves.count - Self.listedAtMost)).foregroundStyle(.secondary)
                            }
                        } header: {
                            Text("organise.moves")
                        }
                    }
                }
            }
            .navigationTitle(said("organise.title.named", folder.name))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("organise.title") { confirming = true }
                        .disabled(problem != nil || planning || (current?.moves.isEmpty ?? true) || library.features.organising)
                }
            }
            .confirmationDialog(
                said("organise.confirm.title", current?.moves.count ?? 0),
                isPresented: $confirming,
                titleVisibility: .visible
            ) {
                Button("organise.title") {
                    guard let plan = current else { return }
                    Self.settings.set(layout, forKey: Self.layoutKey)
                    library.features.organise(folder, plan.moves)
                    dismiss()
                }
            } message: {
                Text(said("organise.confirm.message", folder.name))
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
            guard !Task.isCancelled else { return }
            failedFor = asked
            library.report(error)
        }
    }

    private func shown(_ uri: String) -> String {
        let prefix = library.folders.prefix(of: folder.id) + "/"
        let relative = uri.hasPrefix(prefix) ? String(uri.dropFirst(prefix.count)) : uri
        return said("organise.move.from", relative.split(separator: "/").map { $0.removingPercentEncoding ?? String($0) }.joined(separator: " / "))
    }
}
