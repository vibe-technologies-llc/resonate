import ResonateLogic
import ResonateNative
import SwiftUI

@MainActor
@Observable
final class SelectedSongs {
    private var selection = RowSelection<UInt64>()

    var selecting: Bool { selection.selecting }
    var count: Int { selection.count }
    var chosen: Set<UInt64> { selection.chosen }

    func contains(_ id: UInt64) -> Bool {
        selection.contains(id)
    }

    func picked(from tracks: [Track]) -> [Track] {
        selection.picked(tracks, key: \.id)
    }

    func begin() {
        selection.begin()
    }

    func begin(with id: UInt64) {
        selection.begin(with: id)
    }

    func toggle(_ id: UInt64) {
        selection.toggle(id)
    }

    func selectAll(_ tracks: [Track]) {
        selection.selectAll(tracks.lazy.map(\.id))
    }

    func clear() {
        guard selection.selecting else { return }
        selection.clear()
    }

    func retain(_ tracks: [Track]) {
        guard selection.count > 0 else { return }
        selection.retain(Set(tracks.lazy.map(\.id)))
    }
}

struct SelectableByName: ViewModifier {
    @Environment(Navigation.self) private var navigation
    let track: Track
    let selected: SelectedSongs?

    func body(content: Content) -> some View {
        if let selected {
            content.accessibilityAction(named: said("selection.select-this", track.shownTitle)) {
                navigation.feelSelected()
                selected.begin(with: track.id)
            }
        } else {
            content
        }
    }
}

struct SongSelectionBar: View {
    private static let favouritesReadAtMost = 500

    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(SelectedSongs.self) private var selected
    @ScaledMetric(relativeTo: .title3) private var height: CGFloat = 56

    var body: some View {
        let chosen = selected.chosen
        let loved = library.favouriteEpoch >= 0 && chosen.count <= Self.favouritesReadAtMost && library.areFavourites(chosen)
        HStack(spacing: 0) {
            action(said("common.play"), "play.fill") { library.play($0, at: 0) }
            action(said("song.add-to-playlist"), "text.badge.plus") { navigation.addToPlaylist($0) }
            action(said(loved ? "song.favourite.remove" : "song.favourite.add"), loved ? "heart.slash" : "heart") {
                navigation.feelHeart()
                library.setFavourites($0, !loved)
            }
            more(count: chosen.count)
        }
        .padding(.horizontal, 8)
        .frame(minHeight: height)
        .glassEffect(.regular, in: .capsule)
        .padding(.horizontal, 16)
        .padding(.bottom, 8)
        .disabled(chosen.isEmpty)
    }

    private func action(_ title: String, _ symbol: String, _ act: @escaping @MainActor ([Track]) -> Void) -> some View {
        Button {
            act(taken())
        } label: {
            Label(title, systemImage: symbol)
                .labelStyle(.iconOnly)
                .font(.title3)
                .frame(maxWidth: .infinity)
                .touchTarget()
        }
        .buttonStyle(.plain)
    }

    private func more(count: Int) -> some View {
        Menu {
            Section {
                Button("song.play-next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                    navigation.feelQueued()
                    library.playNext(taken())
                }
                Button("song.add-to-queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                    navigation.feelQueued()
                    library.addToQueue(taken())
                }
            }
            if library.features.holdsAWritableFolder {
                Section {
                    Button(said("deleting.songs.confirm", count), systemImage: "trash", role: .destructive) {
                        navigation.deleting = SongsToDelete(tracks: taken())
                    }
                    if library.features.syncsWithOtherDevices {
                        Button("song.delete-everywhere", systemImage: "trash.slash", role: .destructive) {
                            navigation.deleting = SongsToDelete(tracks: taken(), everywhere: true)
                        }
                    }
                }
            }
        } label: {
            Label("selection.more", systemImage: "ellipsis")
                .labelStyle(.iconOnly)
                .font(.title3)
                .frame(maxWidth: .infinity)
                .touchTarget()
        }
        .buttonStyle(.plain)
    }

    private func taken() -> [Track] {
        let picked = selected.picked(from: library.tracks)
        selected.clear()
        return picked
    }
}
