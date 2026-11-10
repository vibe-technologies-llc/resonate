import ResonateLogic
import ResonateNative
import SwiftUI

struct SavedSearchSheet: View {
    @Environment(\.dismiss) private var dismiss
    let title: String
    let save: (String, SavedSearch) -> Void
    private let initial: SavedSearchDraft
    @State private var draft: SavedSearchDraft
    @State private var discarding = false

    init(title: String, draft: SavedSearchDraft, save: @escaping (String, SavedSearch) -> Void) {
        self.title = title
        self.save = save
        initial = draft
        _draft = State(initialValue: draft)
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("saved-search.name") {
                    TextField("saved-search.name", text: $draft.name)
                }
                Section {
                    TextField("saved-search.text", text: $draft.text)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                } header: {
                    Text("saved-search.text")
                } footer: {
                    Text("saved-search.text.footer")
                }
                Section("saved-search.order") {
                    Picker("saved-search.order", selection: Binding(get: { draft.order }, set: { draft.choose($0) })) {
                        ForEach(SearchOrder.offered, id: \.self) { Text($0.label).tag($0) }
                    }
                    Toggle("saved-search.descending", isOn: $draft.descending)
                }
                Section {
                    TextField("saved-search.limit.none", text: $draft.cap)
                        .keyboardType(.numberPad)
                } header: {
                    Text("saved-search.limit")
                } footer: {
                    Text(said(draft.search == nil ? "saved-search.limit.refused" : "saved-search.limit.footer"))
                    .foregroundStyle(draft.search == nil ? Color.failure : Color.secondary)
                    .announced(draft.search == nil ? said("saved-search.limit.refused") : nil)
                }
            }
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("common.cancel") {
                        if draft == initial { dismiss() } else { discarding = true }
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("common.save") {
                        guard let search = draft.search else { return }
                        save(draft.trimmedName, search)
                        dismiss()
                    }
                    .disabled(!draft.savable)
                }
            }
        }
        .keepingEditsUnlessDiscarded(draft != initial, asking: $discarding) { dismiss() }
        .sheetSized(.long)
        .presentationDetents([.large])
    }
}
