import AVKit
import MediaPlayer
import ResonateNative
import SwiftUI

struct MiniPlayer: View {
    private static let swipeDistance: CGFloat = 40

    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(\.tabViewBottomAccessoryPlacement) private var placement
    @ScaledMetric(relativeTo: .subheadline) private var artworkEdge: CGFloat = 36
    let zoom: Namespace.ID
    @State private var skipped = 0

    var body: some View {
        let track = library.snapshot?.current
        let expanded = placement != .inline
        HStack(spacing: 12) {
            ArtworkView(track: track, corner: 8)
                .frame(width: artworkEdge, height: artworkEdge)
                .matchedTransitionSource(id: PlayerTransition.source, in: zoom)
            VStack(alignment: .leading, spacing: 0) {
                Text(track?.shownTitle ?? "").font(.subheadline.weight(.semibold)).lineLimit(1)
                if expanded {
                    Text(track?.shownArtist ?? "").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            .contentTransition(.opacity)
            .animation(.smooth, value: track?.id)
            .accessibilityElement(children: .combine)
            Spacer(minLength: 0)
            if let track, expanded {
                FavouriteButton(track: track)
            }
            if expanded {
                Button("Previous", systemImage: "backward.fill") { skip(forward: false) }
                    .labelStyle(.iconOnly)
            }
            PlayButton(transport: library.transport)
                .background { MiniProgressRing() }
            Button("Next", systemImage: "forward.fill") { skip(forward: true) }
                .labelStyle(.iconOnly)
        }
        .buttonStyle(.plain)
        .padding(.horizontal, 14)
        .contentShape(.rect)
        .onTapGesture { navigation.nowPlaying = true }
        .simultaneousGesture(
            DragGesture(minimumDistance: 24).onEnded { drag in
                guard abs(drag.translation.width) > max(abs(drag.translation.height), Self.swipeDistance) else { return }
                skip(forward: drag.translation.width < 0)
            }
        )
        .sensoryFeedback(.impact(weight: .light), trigger: skipped)
        .accessibilityAction(named: "Open now playing") { navigation.nowPlaying = true }
        .accessibilityAction(named: "Previous") { skip(forward: false) }
        .accessibilityAction(named: "Next") { skip(forward: true) }
    }

    private func skip(forward: Bool) {
        skipped += 1
        if forward { library.next() } else { library.previous() }
    }
}

extension PlayClock {
    var playedFraction: Double {
        durationMs > 0 ? min(Double(positionMs) / Double(durationMs), 1) : 0
    }
}

struct MiniProgressRing: View {
    private static let lineWidth: CGFloat = 2

    @Environment(Library.self) private var library

    var body: some View {
        let fraction = library.clock.playedFraction
        ZStack {
            Circle()
                .stroke(.quaternary, lineWidth: Self.lineWidth)
            Circle()
                .trim(from: 0, to: fraction)
                .stroke(.tint, style: StrokeStyle(lineWidth: Self.lineWidth, lineCap: .round))
                .rotationEffect(.degrees(-90))
                .animation(.linear(duration: 0.25), value: fraction)
        }
        .padding(Self.lineWidth / 2)
        .accessibilityHidden(true)
    }
}

struct PlayButton: View {
    @Environment(Library.self) private var library
    let transport: Transport
    var large = false
    @State private var pressed = 0

    var body: some View {
        let playingish = transport == .playing || transport == .loading
        Button {
            pressed += 1
            library.toggle()
        } label: {
            Image(systemName: playingish ? "pause.fill" : "play.fill")
                .font(large ? .system(size: 34, weight: .semibold) : .title3)
                .frame(width: large ? 76 : 32, height: large ? 76 : 32)
                .contentTransition(.symbolEffect(.replace))
        }
        .buttonStyle(.plain)
        .modifier(GlassWhen(enabled: large))
        .sensoryFeedback(.impact(weight: .medium), trigger: pressed)
        .accessibilityLabel(playingish ? "Pause" : "Play")
    }
}

struct GlassWhen: ViewModifier {
    let enabled: Bool

    func body(content: Content) -> some View {
        if enabled {
            content.glassEffect(.regular.interactive(), in: .circle)
        } else {
            content
        }
    }
}

enum PlayerMode {
    case artwork
    case lyrics
    case queue
}

struct NowPlayingView: View {
    private static let cover = "cover"

    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    @Environment(\.dismiss) private var dismiss
    @Environment(\.dynamicTypeSize) private var typeSize
    @Namespace private var shared
    @State private var mode = PlayerMode.artwork
    @State private var choosingPlaylist = false

    var body: some View {
        let track = library.snapshot?.current
        NavigationStack {
            Group {
                if mode == .artwork, typeSize.isAccessibilitySize {
                    ScrollView { artworkLayout(track) }
                } else if mode == .artwork {
                    artworkLayout(track)
                } else {
                    focusedLayout(track)
                }
            }
            .padding(.horizontal, 24)
            .background(BackdropView(track: track))
            .safeAreaInset(edge: .top) { StatusBanner() }
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Close", systemImage: "chevron.down") { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    PlayerMoreMenu(track: track, choosingPlaylist: $choosingPlaylist)
                }
            }
            .sheet(isPresented: $choosingPlaylist) {
                if let track { PlaylistPicker(tracks: [track]) }
            }
        }
    }

    private func artworkLayout(_ track: Track?) -> some View {
        VStack(spacing: 20) {
            Spacer(minLength: 0)
            LargeArtwork(track: track)
                .matchedGeometryEffect(id: Self.cover, in: shared)
            PlayerTitles(track: track)
            PlayerScrubber()
            TransportRow()
            SleepCountdown()
            VolumeRow()
            PlayerModes(mode: $mode)
            Spacer(minLength: 0)
        }
    }

    private func focusedLayout(_ track: Track?) -> some View {
        VStack(spacing: 16) {
            CompactTitles(track: track, cover: Self.cover, shared: shared)
                .padding(.top, 8)
            Group {
                switch mode {
                case .lyrics: LyricsPane(track: track, compact: true)
                case .queue, .artwork: UpNextList()
                }
            }
            .frame(maxHeight: .infinity)
            .transition(.opacity)
            PlayerScrubber()
            TransportRow()
            PlayerModes(mode: $mode)
        }
        .padding(.bottom, 8)
    }
}

struct LargeArtwork: View {
    @Environment(Library.self) private var library
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let track: Track?

    var body: some View {
        let playing = library.transport == .playing || library.transport == .loading
        ArtworkView(track: track, corner: 24)
            .aspectRatio(1, contentMode: .fit)
            .shadow(color: .black.opacity(0.3), radius: 28, y: 16)
            .scaleEffect(playing || reduceMotion ? 1 : 0.84)
            .animation(.spring(duration: 0.5, bounce: 0.25), value: playing)
            .padding(.horizontal, 8)
    }
}

struct PlayerTitles: View {
    let track: Track?

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                if let track {
                    SongMenu(track: track) {
                        Text(track.shownTitle)
                            .font(.title2.weight(.bold))
                            .foregroundStyle(Color.primary)
                            .lineLimit(2)
                            .multilineTextAlignment(.leading)
                    }
                    SongMenu(track: track) {
                        Text(track.shownArtist)
                            .font(.title3)
                            .foregroundStyle(Color.secondary)
                    }
                    if !track.album.isEmpty {
                        SongMenu(track: track) {
                            Text(track.album)
                                .font(.subheadline)
                                .foregroundStyle(Color.secondary.opacity(0.8))
                        }
                    }
                } else {
                    Text("Nothing playing")
                        .font(.title2.weight(.bold))
                        .lineLimit(2)
                }
            }
            .lineLimit(1)
            Spacer(minLength: 0)
            if let track { FavouriteButton(track: track).font(.title2) }
        }
    }
}

struct SongMenu<Label: View>: View {
    let track: Track
    @ViewBuilder let label: () -> Label

    var body: some View {
        Menu {
            SongDestinations(track: track)
        } label: {
            label()
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .tint(Color.primary)
        .accessibilityHint("Shows the album, artist and song details")
    }
}

struct SongDestinations: View {
    @Environment(Navigation.self) private var navigation
    let track: Track

    var body: some View {
        if !track.album.isEmpty {
            Button("Go to Album", systemImage: "square.stack") {
                navigation.open(.album(title: track.album, artist: track.groupingArtist))
            }
        }
        Button("Go to Artist", systemImage: "music.mic") {
            navigation.open(.artist(track.artist))
        }
        Button("Song Details", systemImage: "info.circle") {
            navigation.open(.inspector(track))
        }
    }
}

struct CompactTitles: View {
    @ScaledMetric(relativeTo: .headline) private var artworkEdge: CGFloat = 56
    let track: Track?
    let cover: String
    let shared: Namespace.ID

    var body: some View {
        HStack(spacing: 12) {
            ArtworkView(track: track, corner: 8)
                .frame(width: artworkEdge, height: artworkEdge)
                .shadow(color: .black.opacity(0.2), radius: 8, y: 4)
                .matchedGeometryEffect(id: cover, in: shared)
            if let track {
                SongMenu(track: track) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(track.shownTitle).font(.headline).foregroundStyle(Color.primary).lineLimit(1)
                        Text(track.shownArtist).font(.subheadline).foregroundStyle(Color.secondary).lineLimit(1)
                    }
                    .accessibilityElement(children: .combine)
                }
            } else {
                Text("Nothing playing").font(.headline).lineLimit(1)
            }
            Spacer(minLength: 0)
            if let track { FavouriteButton(track: track).font(.title3) }
        }
    }
}

struct PlayerScrubber: View {
    private static let nearEnoughMs: Double = 1500
    private static let holdAtMost: Duration = .seconds(1)
    private static let restingHeight: CGFloat = 7
    private static let grabbedHeight: CGFloat = 14
    private static let touchHeight: CGFloat = 28
    private static let stepMs: Double = 10_000
    private static let slightestMove: CGFloat = 2

    @Environment(Library.self) private var library
    @State private var dragging: Double?
    @State private var grabbedAt: Double?
    @State private var held: Double?
    @State private var grabs = 0

    var body: some View {
        let timing = library.clock
        let duration = Double(max(timing.durationMs, 1))
        let live = Double(timing.positionMs)
        let shown = min(dragging ?? held ?? live, duration)
        let grabbed = dragging != nil

        VStack(spacing: 6) {
            GeometryReader { space in
                let width = max(space.size.width, 1)
                Capsule()
                    .fill(.primary.opacity(0.16))
                    .overlay(alignment: .leading) {
                        Rectangle()
                            .fill(.primary.opacity(grabbed ? 0.95 : 0.7))
                            .frame(width: width * shown / duration)
                    }
                    .clipShape(Capsule())
                    .frame(height: grabbed ? Self.grabbedHeight : Self.restingHeight)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .contentShape(Rectangle())
                    .gesture(drag(across: width, of: duration, from: shown))
            }
            .frame(height: Self.touchHeight)
            .animation(.spring(duration: 0.3, bounce: 0.3), value: grabbed)
            HStack {
                Text(clock(UInt64(shown)))
                Spacer()
                QualityLine(track: library.snapshot?.current)
                Spacer()
                Text("-" + clock(UInt64(max(duration - shown, 0))))
            }
            .font(.caption.monospacedDigit())
            .foregroundStyle(grabbed ? .primary : .secondary)
            .animation(.smooth, value: grabbed)
        }
        .allowsHitTesting(library.loaded)
        .sensoryFeedback(.impact(weight: .light), trigger: grabs)
        .accessibilityElement()
        .accessibilityLabel("Song position")
        .accessibilityValue("\(clock(UInt64(shown))) of \(clock(timing.durationMs))")
        .accessibilityAdjustableAction { direction in
            let step = direction == .increment ? Self.stepMs : -Self.stepMs
            commit(min(max(shown + step, 0), duration))
        }
        .onChange(of: timing.positionMs) { _, now in
            if let target = held, abs(Double(now) - target) < Self.nearEnoughMs { held = nil }
        }
        .task(id: held) {
            guard held != nil else { return }
            try? await Task.sleep(for: Self.holdAtMost)
            if !Task.isCancelled { held = nil }
        }
    }

    private func drag(across width: CGFloat, of duration: Double, from shown: Double) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { change in
                let start = grabbedAt ?? shown
                if grabbedAt == nil {
                    grabbedAt = start
                    grabs += 1
                }
                let moved = Double(change.translation.width / width) * duration
                dragging = min(max(start + moved, 0), duration)
            }
            .onEnded { change in
                grabbedAt = nil
                let target = dragging
                dragging = nil
                guard let target, abs(change.translation.width) >= Self.slightestMove else { return }
                commit(target)
            }
    }

    private func commit(_ target: Double) {
        held = target
        library.seek(to: UInt64(target))
    }
}

struct QualityLine: View {
    @Environment(Library.self) private var library
    let track: Track?

    var body: some View {
        let output = library.clock.outputRate
        let line = Self.line(track, output: output)
        if !line.trimmingCharacters(in: .whitespaces).isEmpty {
            Text(line)
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .minimumScaleFactor(0.7)
                .padding(.horizontal, 7)
                .padding(.vertical, 2)
                .background(.quaternary, in: .capsule)
        }
    }

    private static func line(_ track: Track?, output: UInt32) -> String {
        guard let track else { return " " }
        var parts = [track.codec]
        if let bits = track.bits { parts.append("\(bits)-bit") }
        if track.rate > 0 { parts.append(kilohertz(track.rate)) }
        if output > 0 { parts.append("Output \(kilohertz(output))") }
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }

    private static func kilohertz(_ rate: UInt32) -> String {
        String(format: "%g kHz", Double(rate) / 1000)
    }
}

struct TransportRow: View {
    @Environment(Library.self) private var library
    @State private var skipped = 0

    var body: some View {
        let shuffled = library.snapshot?.shuffle ?? false
        let repeating = library.repeatMode
        GlassEffectContainer(spacing: 24) {
            HStack(spacing: 0) {
                Button("Shuffle", systemImage: "shuffle") { library.setShuffle(!shuffled) }
                    .foregroundStyle(shuffled ? Color.accentColor : .secondary)
                    .accessibilityValue(shuffled ? "On" : "Off")
                    .frame(maxWidth: .infinity)
                Button("Previous", systemImage: "backward.fill") {
                    skipped += 1
                    library.previous()
                }
                .font(.title)
                .frame(maxWidth: .infinity)
                PlayButton(transport: library.transport, large: true)
                Button("Next", systemImage: "forward.fill") {
                    skipped += 1
                    library.next()
                }
                .font(.title)
                .frame(maxWidth: .infinity)
                Button("Repeat", systemImage: repeating.symbol) { library.setRepeat(repeating.following) }
                    .foregroundStyle(repeating == .off ? .secondary : Color.accentColor)
                    .accessibilityValue(repeating.spoken)
                    .frame(maxWidth: .infinity)
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.plain)
        }
        .disabled(!library.loaded)
        .sensoryFeedback(.impact(weight: .light), trigger: skipped)
        .sensoryFeedback(.selection, trigger: shuffled)
        .sensoryFeedback(.selection, trigger: repeating)
    }
}

struct SleepCountdown: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation

    var body: some View {
        if let text = Self.text(library.snapshot?.sleep, remaining: library.clock.sleepRemainingMs) {
            Button {
                navigation.sleepTimer = true
            } label: {
                Label(text, systemImage: "moon.zzz.fill")
                    .font(.footnote.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
            .accessibilityHint("Opens the sleep timer")
        }
    }

    static func text(_ timer: SleepTimer?, remaining: UInt64?) -> String? {
        switch timer {
        case .after: "Pausing in \(clock((remaining ?? 0) + 999))"
        case .endOfTrack: "Pauses at the end of this song"
        case .endOfQueue: "Pauses at the end of the queue"
        case nil: nil
        }
    }
}

struct VolumeRow: View {
    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: "speaker.fill").foregroundStyle(.secondary).accessibilityHidden(true)
            SystemVolume().frame(height: 34)
            Image(systemName: "speaker.wave.3.fill").foregroundStyle(.secondary).accessibilityHidden(true)
        }
    }
}

struct SystemVolume: UIViewRepresentable {
    func makeUIView(context: Context) -> CentredVolume {
        CentredVolume()
    }

    func updateUIView(_ uiView: CentredVolume, context: Context) {}
}

final class CentredVolume: UIView {
    private let volume = MPVolumeView(frame: .zero)

    init() {
        super.init(frame: .zero)
        volume.setValue(false, forKey: "showsRouteButton")
        addSubview(volume)
    }

    required init?(coder: NSCoder) {
        nil
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        volume.frame = bounds
        volume.layoutIfNeeded()
        guard let slider = Self.slider(in: volume) else { return }
        let track = slider.convert(slider.bounds, to: self)

        volume.frame = bounds.offsetBy(dx: 0, dy: bounds.midY - track.midY)
    }

    private static func slider(in view: UIView) -> UISlider? {
        for child in view.subviews {
            if let slider = child as? UISlider { return slider }
            if let nested = slider(in: child) { return nested }
        }
        return nil
    }
}

struct PlayerModes: View {
    @Binding var mode: PlayerMode
    @State private var switched = 0

    var body: some View {
        GlassEffectContainer(spacing: 20) {
            HStack(spacing: 20) {
                toggle(.lyrics, title: "Lyrics", symbol: "quote.bubble")
                RoutePicker()
                    .frame(width: 48, height: 48)
                    .glassEffect(.regular.interactive(), in: .circle)
                    .accessibilityLabel("AirPlay")
                toggle(.queue, title: "Up next", symbol: "list.bullet")
            }
        }
        .sensoryFeedback(.selection, trigger: switched)
    }

    private func toggle(_ target: PlayerMode, title: String, symbol: String) -> some View {
        let active = mode == target
        return Button(title, systemImage: symbol) {
            switched += 1
            withAnimation(.spring(duration: 0.45, bounce: 0.15)) {
                mode = active ? .artwork : target
            }
        }
        .labelStyle(.iconOnly)
        .font(.title3)
        .frame(width: 48, height: 48)
        .foregroundStyle(active ? Color.white : Color.primary)
        .glassEffect(active ? .regular.tint(.accentColor).interactive() : .regular.interactive(), in: .circle)
        .buttonStyle(.plain)
        .accessibilityAddTraits(active ? .isSelected : [])
    }
}

struct PlayerMoreMenu: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let track: Track?
    @Binding var choosingPlaylist: Bool

    var body: some View {
        Menu("More", systemImage: "ellipsis") {
            if let track {
                Section {
                    Button("Add to playlist", systemImage: "text.badge.plus") { choosingPlaylist = true }
                    Button("Song details", systemImage: "info.circle") { navigation.open(.inspector(track)) }
                    Button("Analyse", systemImage: "waveform.path.ecg") { navigation.open(.analysis(track)) }
                }
            }
            Section {
                Button("Sleep timer", systemImage: library.snapshot?.sleep == nil ? "moon.zzz" : "moon.zzz.fill") {
                    navigation.sleepTimer = true
                }
                Button("Equaliser", systemImage: "slider.vertical.3") {
                    navigation.tab = .settings
                    navigation.open(.sound)
                }
                Button("Visualizer", systemImage: "waveform") { navigation.open(.visualizer) }
                Button("Output", systemImage: "hifispeaker") {
                    navigation.tab = .settings
                    navigation.open(.output)
                }
            }
        }
    }
}

struct RoutePicker: UIViewRepresentable {
    func makeUIView(context: Context) -> AVRoutePickerView {
        let picker = AVRoutePickerView()
        picker.prioritizesVideoDevices = false
        return picker
    }

    func updateUIView(_ uiView: AVRoutePickerView, context: Context) {}
}

struct QueueView: View {
    private static let playingRow = "playing"

    @Environment(Library.self) private var library
    @State private var clearing = false

    var body: some View {
        ScrollViewReader { scroller in
            List {
                HandoffRows()
                QueueSections(showsHistory: true, playingRowId: Self.playingRow)
            }
            .listStyle(.plain)
            .onAppear { scroller.scrollTo(Self.playingRow, anchor: .top) }
        }
        .navigationTitle("Queue")
        .refreshable { await library.rereadQueue() }
        .toolbar {
            if library.loaded {
                ToolbarItem(placement: .topBarTrailing) { SleepTimerButton() }
                ToolbarSpacer(.fixed, placement: .topBarTrailing)
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button("Clear", systemImage: "trash") { clearing = true }
                    .disabled(library.queue.count <= 1)
            }
            ToolbarItem(placement: .topBarTrailing) { EditButton() }
        }
        .confirmationDialog("Clear the queue?", isPresented: $clearing, titleVisibility: .visible) {
            Button("Clear queue", role: .destructive) { library.clearQueue() }
        } message: {
            Text("Songs up next and in your history are removed. The playing song keeps playing.")
        }
    }
}

struct UpNextList: View {
    var body: some View {
        List {
            QueueSections(showsHistory: false, playingRowId: nil)
                .listRowBackground(Color.clear)
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
    }
}

struct QueueSections: View {
    @Environment(Library.self) private var library
    let showsHistory: Bool
    let playingRowId: String?
    @State private var edited = 0

    var body: some View {
        let rows = library.queue
        let playing = library.playingQueueRow
        Group {
            if rows.isEmpty {
                Text("Your queue is empty").foregroundStyle(.secondary)
            } else if let playing {
                if showsHistory, playing > 0 {
                    Section("History") { rowsOf(rows, from: 0, to: playing) }
                }
                if showsHistory {
                    Section("Now playing") {
                        QueueRow(row: rows[playing], edited: $edited)
                            .id(playingRowId)
                    }
                }
                Section("Up next") {
                    if playing + 1 < rows.count {
                        rowsOf(rows, from: playing + 1, to: rows.count)
                    } else {
                        Text("Nothing up next").foregroundStyle(.secondary)
                    }
                }
            } else {
                Section("Up next") { rowsOf(rows, from: 0, to: rows.count) }
            }
        }
        .sensoryFeedback(.selection, trigger: edited)
    }

    private func rowsOf(_ rows: [Track], from start: Int, to end: Int) -> some View {
        ForEach(Array(rows[start..<end]), id: \.queueId) { row in
            QueueRow(row: row, edited: $edited)
        }
        .onMove { offsets, destination in
            guard let moved = offsets.first else { return }
            edited += 1
            library.moveInQueue(from: start + moved, to: start + destination)
        }
        .onDelete { offsets in
            edited += 1
            for offset in offsets.sorted(by: >) { library.removeFromQueue(rows[start + offset]) }
        }
    }
}

struct QueueRow: View {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let row: Track
    @Binding var edited: Int
    @State private var choosingPlaylist = false

    var body: some View {
        let favourite = library.isFavourite(row)
        TrackRow(track: row, current: row.queueId == library.currentQueueId, ownsMenus: false) {
            library.jump(to: row)
        }
        .swipeActions(edge: .trailing) {
            Button("Remove from queue", systemImage: "minus.circle", role: .destructive) {
                edited += 1
                library.removeFromQueue(row)
            }
        }
        .contextMenu {
            Button("Remove from queue", systemImage: "minus.circle", role: .destructive) {
                edited += 1
                library.removeFromQueue(row)
            }
            Button(favourite ? "Remove from favourites" : "Add to favourites", systemImage: favourite ? "heart.slash" : "heart") {
                library.setFavourite(row, !favourite)
            }
            Button("Add to playlist", systemImage: "text.badge.plus") { choosingPlaylist = true }
            Button("Song details", systemImage: "info.circle") { navigation.open(.inspector(row)) }
        }
        .sheet(isPresented: $choosingPlaylist) { PlaylistPicker(tracks: [row]) }
    }
}

struct HandoffRows: View {
    @Environment(Library.self) private var library

    var body: some View {
        ForEach(library.features.syncHandoffs, id: \.device) { handoff in
            Button {
                library.features.continueFrom(handoff)
            } label: {
                Label(
                    "Continue \(counted(Int(handoff.songs), "song")) from \(handoff.device)\(handoff.playing ? " · playing" : "")",
                    systemImage: "arrow.triangle.2.circlepath"
                )
            }
        }
    }
}

struct SleepTimerSheet: View {
    private static let minutes: [UInt32] = [5, 15, 30, 45, 60, 90]

    @Environment(Library.self) private var library
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let timer = library.snapshot?.sleep
        NavigationStack {
            List {
                Section {
                    SleepTimerState()
                }
                Section {
                    ForEach(Self.minutes, id: \.self) { minutes in
                        Button("\(minutes) minutes") { set(.after(seconds: minutes * 60)) }
                    }
                    Button("End of song") { set(.endOfTrack) }
                    Button("End of queue") { set(.endOfQueue) }
                }
                if timer != nil {
                    Button("Cancel timer", role: .destructive) { set(nil) }
                }
            }
            .navigationTitle("Sleep timer")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .close) { dismiss() }
                }
            }
        }
    }

    private func set(_ timer: SleepTimer?) {
        library.request { try $0.setSleep(timer: timer) }
        dismiss()
    }
}

struct SleepTimerState: View {
    @Environment(Library.self) private var library

    var body: some View {
        if let text = SleepCountdown.text(library.snapshot?.sleep, remaining: library.clock.sleepRemainingMs) {
            Text(text).font(.headline.monospacedDigit())
        } else {
            Text("No timer set").foregroundStyle(.secondary)
        }
    }
}
