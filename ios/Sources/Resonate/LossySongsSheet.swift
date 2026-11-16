import ResonateLogic
import ResonateNative
import SwiftUI

enum LossySongsShown {
    case reading
    case found([LossySong])
    case failed(String)
}

struct LossySongsSheet: View {
    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss
    let id: UInt64
    let name: String
    @State private var shown = LossySongsShown.reading
    @State private var ticked: Set<UInt64> = []
    @State private var asked = 0
    @State private var confirming = false
    @State private var replacing = false

    var body: some View {
        let songs = found
        let deletes = LossySongs.deletes(songs, ticked)
        NavigationStack {
            List {
                Section {
                    content
                } header: {
                    Text(name).textCase(nil)
                } footer: {
                    Text("lossy.detail")
                }
            }
            .navigationTitle("lossy.title")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(said("lossy.replace", ticked.count)) {
                        if deletes > 0 { confirming = true } else { replace() }
                    }
                    .disabled(ticked.isEmpty || replacing)
                }
            }
            .confirmationDialog(said("lossy.delete.title", deletes), isPresented: $confirming, titleVisibility: .visible) {
                Button(said("lossy.replace", ticked.count), role: .destructive) { replace() }
            } message: {
                Text("lossy.delete.message")
            }
        }
        .task(id: asked) { await read() }
    }

    private var found: [LossySong] {
        if case .found(let songs) = shown { return songs }
        return []
    }

    @ViewBuilder
    private var content: some View {
        switch shown {
        case .reading:
            ProgressView().accessibilityLabel("lossy.reading").frame(maxWidth: .infinity)
        case .found(let songs) where songs.isEmpty:
            ContentUnavailableView("lossy.none", systemImage: "checkmark.circle", description: Text("lossy.none.detail"))
                .listRowSeparator(.hidden)
        case .found(let songs):
            ForEach(songs, id: \.song.id) { lossy in
                LossySongRow(lossy: lossy, ticked: ticked.contains(lossy.song.id)) {
                    if ticked.contains(lossy.song.id) {
                        ticked.remove(lossy.song.id)
                    } else {
                        ticked.insert(lossy.song.id)
                    }
                }
                .disabled(replacing || !LossySongs.tickable(lossy))
            }
        case .failed(let failure):
            ReadFailure(text: failure) { asked += 1 }
        }
    }

    private func read() async {
        let id = id
        do {
            let native = try await library.player()
            let songs = try await Work.aside { try native.lossySongsOf(playlist: id) }
            if case .found = shown {
                ticked = LossySongs.stillTicked(songs, ticked)
            } else {
                ticked = LossySongs.ticked(songs)
            }
            shown = .found(songs)
        } catch {
            shown = .failed(explained(error))
        }
    }

    private func replace() {
        let chosen = Array(ticked)
        replacing = true
        Task {
            await library.replaceLossySongs(chosen)
            replacing = false
            asked += 1
        }
    }
}

private struct LossySongRow: View {
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 44
    let lossy: LossySong
    let ticked: Bool
    let tick: () -> Void

    var body: some View {
        Button(action: tick) {
            HStack(spacing: 12) {
                ArtworkView(track: lossy.song, corner: 6)
                    .frame(width: artworkEdge, height: artworkEdge)
                VStack(alignment: .leading, spacing: 2) {
                    Text(lossy.song.shownTitle).titleLines()
                    Text(LossySongs.line(lossy)).font(.subheadline).foregroundStyle(.secondary).titleLines(2)
                }
                Spacer(minLength: 0)
                Image(systemName: ticked ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(ticked ? Color.worn : Color.secondary)
                    .accessibilityHidden(true)
            }
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .accessibilityValue(said(ticked ? "lossy.ticked" : "lossy.unticked"))
        .accessibilityHint(said("lossy.replace.hint"))
    }
}
