import ResonateLogic
import ResonateNative
import SwiftUI

extension Track {
    var albumPage: Destination? { isServed ? nil : albumKey(album: album, artist: artist, albumArtist: albumArtist).map(Destination.album) }
}

struct MoreRows: View {
    let widen: () -> Void

    var body: some View {
        ProgressView()
            .frame(maxWidth: .infinity)
            .accessibilityLabel("songs.more.reading")
            .listRowSeparator(.hidden)
            .onAppear(perform: widen)
    }
}

struct OrderOption<Order: Hashable>: View {
    @Binding var selection: Order
    let order: Order
    let title: String
    let symbol: String

    var body: some View {
        let chosen = selection == order
        Button {
            selection = order
        } label: {
            Label(title, systemImage: chosen ? "checkmark" : symbol)
        }
        .accessibilityAddTraits(chosen ? .isSelected : [])
    }
}

struct DescendingOption: View {
    @Binding var descending: Bool

    var body: some View {
        Section {
            Toggle("filter.descending", systemImage: "arrow.down", isOn: $descending)
        }
    }
}

extension CollectionOrder {
    var symbol: String {
        switch self {
        case .alphabetical: "textformat"
        case .artist: "music.mic"
        case .year: "calendar"
        case .added: "clock"
        case .plays: "chart.bar"
        }
    }

    var title: String {
        switch self {
        case .alphabetical: said("collections.order.name")
        case .artist: said("collections.order.artist")
        case .year: said("collections.order.year")
        case .added: said("collections.order.added")
        case .plays: said("collections.order.plays")
        }
    }
}

struct CollectionOrderMenu: View {
    @Environment(Library.self) private var library
    let shelf: LibraryShelf

    var body: some View {
        @Bindable var library = library
        let order = shelf == .albums ? $library.albumOrder : $library.artistOrder
        Menu {
            ForEach(shelf.orders, id: \.self) { offered in
                OrderOption(selection: order, order: offered, title: offered.title, symbol: offered.symbol)
            }
        } label: {
            ShelfChip(title: order.wrappedValue.title, symbol: "arrow.up.arrow.down", opens: true)
        }
        .accessibilityLabel("filter.sort-by")
        .accessibilityValue(order.wrappedValue.title)
    }
}

struct SongOrderMenu: View {
    @Binding var ordering: SongOrdering
    var asChip = false

    var body: some View {
        let title = FilterChip.orderNames.first { $0.0 == ordering.order }?.1 ?? ""
        Menu {
            ForEach(FilterChip.orderNames, id: \.1) { order, title in
                OrderOption(selection: $ordering.order, order: order, title: title, symbol: order.symbol)
            }
            DescendingOption(descending: $ordering.descending)
        } label: {
            if asChip {
                ShelfChip(title: title, symbol: ordering.descending ? "arrow.down" : "arrow.up", opens: true)
            } else {
                Label("filter.sort-by", systemImage: "arrow.up.arrow.down")
            }
        }
        .accessibilityLabel("filter.sort-by")
        .accessibilityValue(ordering.descending ? said("order.descending", title) : said("order.ascending", title))
    }
}

struct PlaylistOrderMenu: View {
    @Binding var order: PlaylistOrder

    var body: some View {
        Menu {
            ForEach(PlaylistOrder.allCases, id: \.self) { option in
                OrderOption(selection: $order, order: option, title: option.title, symbol: option.symbol)
            }
        } label: {
            ShelfChip(title: order.title, symbol: "arrow.up.arrow.down", opens: true)
        }
        .accessibilityLabel("filter.sort-by")
        .accessibilityValue(order.title)
    }
}

struct ShelfChip: View {
    @ScaledMetric(relativeTo: .subheadline) private var height: CGFloat = 34
    let title: String
    let symbol: String
    var opens = false
    var lit = false

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: symbol)
                .font(.footnote.weight(.semibold))
                .accessibilityHidden(true)
            Text(title).titleLines()
            if opens {
                Image(systemName: "chevron.down")
                    .font(.caption2.weight(.bold))
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
            }
        }
        .font(.subheadline.weight(.medium))
        .foregroundStyle(lit ? AnyShapeStyle(.tint) : AnyShapeStyle(.primary))
        .padding(.horizontal, 12)
        .frame(minHeight: height)
        .background(.quaternary, in: .capsule)
        .contentShape(.capsule)
    }
}

struct FilterChips: View {
    @Environment(Library.self) private var library

    var body: some View {
        let filter = library.filter
        let chips = FilterChip.parts(of: filter)
        if !chips.isEmpty {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    ForEach(chips) { chip in
                        Button { library.setFilter(chip.cleared(from: filter)) } label: {
                            FilterToken(label: chip.label)
                        }
                        .buttonStyle(.borderless)
                        .touchTarget()
                        .accessibilityLabel(said("filter.chip.remove", chip.label))
                        .accessibilityInputLabels([chip.label, said("filter.chip.remove.spoken", chip.label)])
                    }
                    Button("library.filters.clear") { library.setFilter(chips.reduce(filter) { $1.cleared(from: $0) }) }
                        .font(.footnote.weight(.medium))
                        .buttonStyle(.borderless)
                        .touchTarget()
                }
                .padding(.horizontal, 16)
            }
            .scrollClipDisabled()
            .listRowInsets(EdgeInsets(top: 0, leading: 0, bottom: 6, trailing: 0))
        }
    }
}

struct FilterToken: View {
    let label: String

    var body: some View {
        HStack(spacing: 4) {
            Text(label).titleLines()
            Image(systemName: "xmark")
                .font(.caption2.weight(.bold))
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
        }
        .font(.footnote.weight(.medium))
        .foregroundStyle(.primary)
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(.quaternary, in: .capsule)
        .contentShape(.capsule)
    }
}

struct FilterButton: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation

    var body: some View {
        let parts = FilterChip.parts(of: library.filter).count
        Button {
            navigation.filters = true
        } label: {
            ShelfChip(
                title: parts == 0 ? said("library.filters") : said("library.filters.counted", parts),
                symbol: "line.3.horizontal.decrease",
                lit: parts > 0
            )
        }
        .buttonStyle(.plain)
        .accessibilityHint("filter.title")
    }
}

struct EmptyLibrary: View {
    @ScaledMetric(relativeTo: .largeTitle) private var mark: CGFloat = 72
    @Environment(Navigation.self) private var navigation

    var body: some View {
        VStack(spacing: 12) {
            ResonateMark()
                .fill(.tint)
                .frame(width: mark, height: mark)
                .accessibilityHidden(true)
            Text("songs.empty.title").font(.title2.weight(.semibold))
            Text("songs.empty.detail")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
            Button("songs.empty.choose-folder") { navigation.choosingFolder = true }
                .buttonStyle(.glassProminent)
                .padding(.top, 8)
        }
        .frame(maxWidth: .infinity)
        .padding(32)
        .listRowSeparator(.hidden)
    }
}

enum CollectionRowMeasure {
    case collection
    case song
}

struct CollectionRow: View {
    @ScaledMetric(relativeTo: .body) private var collectionEdge: CGFloat = 56
    @ScaledMetric(relativeTo: .body) private var songEdge: CGFloat = 48
    let track: Track?
    let title: AttributedString
    let detail: String
    let circle: Bool
    let disclosed: Bool
    let measure: CollectionRowMeasure

    init(
        track: Track?,
        title: AttributedString,
        detail: String,
        circle: Bool = false,
        disclosed: Bool = false,
        measure: CollectionRowMeasure = .collection
    ) {
        self.track = track
        self.title = title
        self.detail = detail
        self.circle = circle
        self.disclosed = disclosed
        self.measure = measure
    }

    init(
        track: Track?,
        title: String,
        detail: String,
        circle: Bool = false,
        disclosed: Bool = false,
        measure: CollectionRowMeasure = .collection
    ) {
        self.init(track: track, title: AttributedString(title), detail: detail, circle: circle, disclosed: disclosed, measure: measure)
    }

    var body: some View {
        let asSong = measure == .song
        let artworkEdge = asSong ? songEdge : collectionEdge
        HStack(spacing: asSong ? 12 : 14) {
            ArtworkView(track: track, corner: asSong ? 8 : 10, circle: circle)
                .frame(width: artworkEdge, height: artworkEdge)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.body.weight(.medium)).titleLines()
                Text(detail).font(.subheadline).foregroundStyle(.secondary).titleLines()
            }
            Spacer(minLength: 0)
            if disclosed {
                Image(systemName: "chevron.right")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
            }
        }
        .contentShape(.rect)
        .accessibilityElement(children: .combine)
    }
}

struct TrackRow: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(SelectedSongs.self) private var selected: SelectedSongs?
    let track: Track
    var current: Bool?
    var numbered: UInt32?
    var timed = false
    var showsAlbum = false
    var query: String?
    var ownsMenus = true
    let play: () -> Void

    var body: some View {
        if let selected, selected.selecting {
            selectable(in: selected)
        } else {
            playable
        }
    }

    private func selectable(in selected: SelectedSongs) -> some View {
        let chosen = selected.contains(track.id)
        return HStack(spacing: 12) {
            PlayableTrack(
                track: track,
                title: title,
                detail: detail,
                current: current,
                numbered: numbered,
                timed: timed,
                play: { selected.toggle(track.id) }
            )
            .accessibilityAddTraits(chosen ? .isSelected : [])
            Button {
                selected.toggle(track.id)
            } label: {
                Image(systemName: chosen ? "checkmark.circle.fill" : "circle")
                    .font(.title3)
                    .foregroundStyle(chosen ? AnyShapeStyle(.tint) : AnyShapeStyle(.tertiary))
                    .touchTarget()
            }
            .buttonStyle(.plain)
            .accessibilityHidden(true)
        }
    }

    private var playable: some View {
        HStack(spacing: 12) {
            PlayableTrack(
                track: track,
                title: title,
                detail: detail,
                current: current,
                numbered: numbered,
                timed: timed,
                play: play
            )
            FavouriteButton(track: track)
        }
        .swipeActions(edge: .leading, allowsFullSwipe: true) {
            Button("song.play-next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                navigation.feelQueued()
                library.playNext([track])
            }
            .tint(.indigo)
            Button("song.add-to-queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                navigation.feelQueued()
                library.addToQueue([track])
            }
            .tint(.orange)
        }
        .modifier(StandardTrackMenus(track: track, enabled: ownsMenus, selected: selected))
        .accessibilityAction(named: "song.details") { navigation.open(.inspector(track)) }
        .modifier(SelectableByName(track: track, selected: selected))
    }

    private var title: AttributedString {
        query.map { lit(track.shownTitle, by: $0, in: .title) } ?? AttributedString(track.shownTitle)
    }

    private var detail: AttributedString? {
        if let query {
            var shown = lit(track.shownArtist, by: query, in: .artist)
            if !track.album.isEmpty {
                shown.append(AttributedString(" · "))
                shown.append(lit(track.album, by: query, in: .album))
            }
            return shown
        }
        if showsAlbum {
            return AttributedString(track.album.isEmpty ? clock(track.durationMs) : "\(track.album) · \(clock(track.durationMs))")
        }
        guard timed else { return AttributedString("\(track.shownArtist) · \(clock(track.durationMs))") }
        return AttributedString(track.shownArtist)
    }
}

struct PlayableTrack: View {
    @Environment(Library.self) private var library
    @ScaledMetric(relativeTo: .body) private var artworkEdge: CGFloat = 48
    @ScaledMetric(relativeTo: .callout) private var numberWidth: CGFloat = 28
    let track: Track
    let title: AttributedString
    let detail: AttributedString?
    let current: Bool?
    let numbered: UInt32?
    let timed: Bool
    let play: () -> Void

    var body: some View {
        let playingHere = current ?? (library.currentId == track.id)
        Button(action: play) {
            HStack(spacing: 12) {
                leading(playingHere)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(.body.weight(playingHere ? .semibold : .regular))
                        .foregroundStyle(playingHere ? Color.worn : Color.primary)
                        .titleLines()
                    if let detail {
                        Text(detail)
                            .font(.subheadline)
                            .secondaryInk()
                            .titleLines()
                    }
                }
                Spacer(minLength: 0)
                if timed {
                    Text(clock(track.durationMs))
                        .font(.subheadline.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
            }
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .accessibilityValue(playingHere ? said("song.now-playing") : "")
    }

    @ViewBuilder
    private func leading(_ playingHere: Bool) -> some View {
        if let numbered {
            ZStack {
                if playingHere {
                    NowPlayingIndicator(ink: .worn)
                } else {
                    Text("\(numbered)").font(.callout.monospacedDigit()).foregroundStyle(.secondary)
                }
            }
            .frame(width: numberWidth)
        } else {
            ArtworkView(track: track, corner: 8)
                .frame(width: artworkEdge, height: artworkEdge)
                .overlay {
                    if playingHere {
                        NowPlayingIndicator(ink: .white)
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                            .background(.black.opacity(0.35), in: .rect(cornerRadius: 8))
                    }
                }
        }
    }
}

struct StandardTrackMenus: ViewModifier {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let track: Track
    let enabled: Bool
    var selected: SelectedSongs?

    func body(content: Content) -> some View {
        if enabled {
            let favourite = library.isFavourite(track)
            content
                .swipeActions(edge: .trailing) {
                    Button(said(favourite ? "song.favourite.remove" : "song.favourite.add"), systemImage: favourite ? "heart.slash" : "heart") {
                        library.setFavourite(track, !favourite)
                    }
                    .tint(.pink)
                    Button("song.add-to-playlist", systemImage: "text.badge.plus") { navigation.addToPlaylist([track]) }
                        .tint(.blue)
                }
                .contextMenu { TrackMenu(track: track, selected: selected) }
        } else {
            content
        }
    }
}

struct NowPlayingIndicator: View {
    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.scenePhase) private var phase
    @ScaledMetric(relativeTo: .callout) private var height: CGFloat = 14
    let ink: Color

    var body: some View {
        PlayingBarsLayers(
            features: library.features,
            following: phase == .active && library.transport == .playing && !reduceMotion,
            calm: reduceMotion,
            ink: ink
        )
        .frame(width: PlayingBarsShape(height: height).width, height: height)
        .accessibilityHidden(true)
    }
}

struct TrackMenu: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let track: Track
    var selected: SelectedSongs?

    var body: some View {
        let favourite = library.isFavourite(track)
        if let selected {
            Section {
                Button("common.select", systemImage: "checkmark.circle") {
                    navigation.feelSelected()
                    selected.begin(with: track.id)
                }
            }
        }
        Section {
            Button("song.play-next", systemImage: "text.line.first.and.arrowtriangle.forward") {
                navigation.feelQueued()
                library.playNext([track])
            }
            Button("song.add-to-queue", systemImage: "text.line.last.and.arrowtriangle.forward") {
                navigation.feelQueued()
                library.addToQueue([track])
            }
        }
        Section {
            Button(said(favourite ? "song.favourite.remove" : "song.favourite.add"), systemImage: favourite ? "heart.slash" : "heart") {
                library.setFavourite(track, !favourite)
            }
            Button("song.add-to-playlist", systemImage: "text.badge.plus") { navigation.addToPlaylist([track]) }
        }
        Section {
            SongPages(track: track)
        }
    }
}

struct SongPages: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let track: Track

    var body: some View {
        if !track.isServed {
            pages
        }
    }

    @ViewBuilder
    private var pages: some View {
        Button("song.details", systemImage: "info.circle") { navigation.open(.inspector(track)) }
        ShareSongButton(track: track)
        Button("song.find-another-version", systemImage: "arrow.left.arrow.right") {
            navigation.findVersions(track)
        }
        if library.features.withVariants.contains(track.id) {
            Button("song.variants", systemImage: "square.stack.3d.up") { navigation.chooseVariant(track) }
        }
        if library.features.tagsWritable(track) {
            Button("song.edit-tags", systemImage: "pencil") { navigation.editTags([track]) }
        }
        Button("tools.lyrics", systemImage: "quote.bubble") { navigation.open(.lyrics(track)) }
        Button("song.analyse", systemImage: "waveform.path.ecg") { navigation.open(.analysis(track)) }
        if let albumPage = track.albumPage {
            Button("song.go-to-album", systemImage: "square.stack") { navigation.open(albumPage) }
        }
        if !track.artist.isEmpty {
            Button("song.go-to-artist", systemImage: "music.mic") { navigation.open(.artist(track.artist)) }
        }
        if library.features.songDeletable(track) {
            Button("song.delete", systemImage: "trash", role: .destructive) {
                navigation.deleting = SongsToDelete(tracks: [track])
            }
            if library.features.syncsWithOtherDevices {
                Button("song.delete-everywhere", systemImage: "trash.slash", role: .destructive) {
                    navigation.deleting = SongsToDelete(tracks: [track], everywhere: true)
                }
            }
        }
    }
}

struct PlaylistPickerHost: ViewModifier {
    @Environment(Navigation.self) private var navigation
    let insidePlayer: Bool

    func body(content: Content) -> some View {
        content
            .sheet(item: Binding(
                get: { navigation.nowPlaying == insidePlayer ? navigation.addingToPlaylist : nil },
                set: { if $0 == nil { navigation.addingToPlaylist = nil } }
            )) { PlaylistPicker(tracks: $0.tracks).sheetSized() }
            .background { FeltFeedback() }
    }
}

private struct FeltFeedback: View {
    @Environment(Navigation.self) private var navigation

    var body: some View {
        Color.clear
            .sensoryFeedback(.success, trigger: navigation.queuedFelt)
            .sensoryFeedback(.selection, trigger: navigation.heartFelt)
            .sensoryFeedback(.success, trigger: navigation.landedFelt)
            .sensoryFeedback(.selection, trigger: navigation.selectedFelt)
            .accessibilityHidden(true)
    }
}
