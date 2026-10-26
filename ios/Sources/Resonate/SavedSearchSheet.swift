import ResonateLogic
import ResonateNative
import SwiftUI

struct SavedSearchSheet: View {
    @Environment(\.dismiss) private var dismiss
    let title: String
    let save: (String, SavedSearch) -> Void
    @State private var draft: SavedSearchDraft

    init(title: String, draft: SavedSearchDraft, save: @escaping (String, SavedSearch) -> Void) {
        self.title = title
        self.save = save
        _draft = State(initialValue: draft)
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("Playlist name") {
                    TextField("Playlist name", text: $draft.name)
                }
                Section {
                    TextField("Search", text: $draft.text)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                } header: {
                    Text("Search")
                } footer: {
                    Text("Blank asks every song.")
                }
                Section("Order") {
                    Picker("Order", selection: Binding(get: { draft.order }, set: { draft.choose($0) })) {
                        ForEach(SearchOrder.offered, id: \.self) { Text($0.label).tag($0) }
                    }
                    Toggle("Descending", isOn: $draft.descending)
                }
                Section {
                    TextField("No limit", text: $draft.cap)
                        .keyboardType(.numberPad)
                } header: {
                    Text("Limit")
                } footer: {
                    Text(
                        draft.search == nil
                            ? "A whole number above zero, or blank for every row."
                            : "Blank keeps every row."
                    )
                    .foregroundStyle(draft.search == nil ? Color.red : Color.secondary)
                }
            }
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        guard let search = draft.search else { return }
                        save(draft.trimmedName, search)
                        dismiss()
                    }
                    .disabled(!draft.savable)
                }
            }
        }
        .presentationDetents([.large])
    }
}
