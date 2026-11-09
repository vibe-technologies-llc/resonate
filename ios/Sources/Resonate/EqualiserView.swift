import Foundation
import ResonateLogic
import ResonateNative
import SwiftUI
import UniformTypeIdentifiers

struct EqualiserView: View {
    @Environment(Library.self) private var library
    @State private var draft = EqualiserDraft()
    @State private var chosen: Int?
    @State private var response = EqualiserResponse()
    @State private var settling: Task<Void, Never>?
    @State private var confirmingReset = false
    static let limits = equaliserLimits()
    static let presets = equaliserPresets()
    private static let octaves = tenBandCurve(preampDb: 0, gainsDb: [])
    private static let settle: Duration = .seconds(1)

    var body: some View {
        let committed = library.preferences?.equaliser
        ThemedForm {
            if let settings = library.preferences {
                let base = draft.shown(over: settings.equaliser)
                let shaped = EqualiserShown.curve(base, octaves: Self.octaves)
                let selected = BandSelection.shown(chosen, among: shaped.bands.count)
                EqualiserSwitches(enabled: settings.equaliserEnabled, summary: settings.equaliser.summary)
                Section {
                    GraphFrame {
                        EqualiserGraph(
                            curve: shaped,
                            response: response,
                            limits: Self.limits,
                            selected: selected,
                            choose: { chosen = $0 },
                            edit: { draft.hold($0) },
                            commit: { edited($0) },
                            discard: { draft.drop() }
                        )
                    }
                    .listRowInsets(EdgeInsets(top: 8, leading: 8, bottom: 4, trailing: 8))
                    BandChips(
                        count: shaped.bands.count,
                        selected: selected,
                        full: shaped.bands.count >= Int(Self.limits.bandsAtMost),
                        choose: { chosen = $0 },
                        add: { add(to: shaped) }
                    )
                    .listRowInsets(EdgeInsets(top: 4, leading: 0, bottom: 4, trailing: 16))
                    if let selected {
                        BandCard(
                            band: shaped.bands[selected],
                            number: selected + 1,
                            limits: Self.limits,
                            moving: { draft.hold(shaped.replacing(selected, with: $0)) },
                            change: { edited(shaped.replacing(selected, with: $0)) },
                            remove: { remove(selected, from: shaped) }
                        )
                        .id(selected)
                    }
                }
                PreampSection(
                    curve: base,
                    limits: Self.limits,
                    moving: { preamp in draft.hold(with(base, preamp: preamp)) },
                    change: { preamp in edited(with(base, preamp: preamp)) },
                    fit: { fit(base) }
                )
                PresetsSection(presets: Self.presets, chosen: shaped.presetName, choose: apply)
                CorrectionsSection()
            }
        }
        .navigationTitle("equaliser.title")
        .background { ResponseTracing(curve: committed.map { draft.shown(over: $0) }, response: response) }
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Menu("equaliser.more", systemImage: "ellipsis") {
                    Button("equaliser.reset", systemImage: "arrow.counterclockwise", role: .destructive) { confirmingReset = true }
                        .disabled(committed == .flat)
                }
            }
        }
        .onChange(of: committed) { _, now in draft.arrived(now) }
        .confirmationDialog("equaliser.reset.question", isPresented: $confirmingReset, titleVisibility: .visible) {
            Button("equaliser.reset", role: .destructive) { reset() }
        } message: {
            Text("equaliser.reset.message")
        }
    }

    private func with(_ curve: EqualiserCurve, preamp: Float) -> EqualiserCurve {
        var next = curve
        next.preampDb = preamp
        return next
    }

    private func commit(_ curve: EqualiserCurve, switchingOn: Bool) {
        draft.release(curve)
        settling?.cancel()
        settling = Task {
            try? await Task.sleep(for: Self.settle)
            guard !Task.isCancelled else { return }
            draft.settle(curve)
        }
        library.updatePreferences { current in
            var next = current
            next.equaliser = curve
            if switchingOn { next.equaliserEnabled = true }
            return next
        }
    }

    private func edited(_ curve: EqualiserCurve) {
        var next = curve
        next.name = ""
        commit(next, switchingOn: true)
    }

    private func apply(_ preset: EqualiserPreset) {
        chosen = nil
        commit(preset.curve, switchingOn: true)
    }

    private func add(to curve: EqualiserCurve) {
        guard let band = BandPlacement.added(to: curve, limits: Self.limits) else { return }
        var next = curve
        next.bands.append(band)
        chosen = next.bands.count - 1
        edited(next)
    }

    private func remove(_ index: Int, from curve: EqualiserCurve) {
        guard curve.bands.indices.contains(index) else { return }
        var next = curve
        next.bands.remove(at: index)
        chosen = BandSelection.afterRemoving(index, leaving: next.bands.count)
        edited(next)
    }

    private func reset() {
        chosen = nil
        commit(.flat, switchingOn: false)
    }

    private func fit(_ curve: EqualiserCurve) {
        let rate = library.clock.outputRate
        let asked: UInt32? = rate > 0 ? rate : nil
        Task {
            do {
                let player = try await library.player()
                let fitted = try await Work.aside { try player.fittedPreamp(curve: curve, rateHz: asked) }
                edited(with(curve, preamp: fitted))
                let measured = try await Work.aside { try player.fittedPreampToTheQueue(curve: curve) }
                guard let measured, let shown = library.preferences.map({ draft.shown(over: $0.equaliser) }),
                      shown.bands == curve.bands, shown.preampDb == fitted
                else { return }
                edited(with(shown, preamp: measured))
            } catch {
                library.report(error)
            }
        }
    }
}

private struct ResponseTracing: View {
    @Environment(Library.self) private var library
    let curve: EqualiserCurve?
    let response: EqualiserResponse

    private struct Traced: Equatable {
        let curve: EqualiserCurve?
        let rate: UInt32
    }

    var body: some View {
        Color.clear
            .accessibilityHidden(true)
            .onChange(of: Traced(curve: curve, rate: library.clock.outputRate), initial: true) { _, traced in
                guard let curve = traced.curve else { return }
                response.follow(curve, rate: traced.rate > 0 ? traced.rate : nil)
            }
    }
}

private struct EqualiserSwitches: View {
    @Environment(Library.self) private var library
    let enabled: Bool
    let summary: String

    var body: some View {
        Section {
            Toggle(isOn: Binding(
                get: { enabled },
                set: { on in library.updatePreferences { var next = $0; next.equaliserEnabled = on; return next } }
            )) {
                ToolRow(title: said("equaliser.title"), detail: enabled ? summary : said("common.off"), symbol: "slider.vertical.3")
            }
            if let route = library.route {
                let bound = library.equaliserBound
                Toggle(isOn: Binding(get: { bound }, set: { library.bindEqualiser($0) })) {
                    ToolRow(
                        title: said("equaliser.only-for", route.name),
                        detail: said(bound ? "equaliser.only-for.bound" : "equaliser.only-for.shared"),
                        symbol: "hifispeaker"
                    )
                }
            }
        }
    }
}

struct BandChips: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let count: Int
    let selected: Int?
    let full: Bool
    let choose: (Int) -> Void
    let add: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            ScrollViewReader { proxy in
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 6) {
                        ForEach(0..<count, id: \.self) { index in
                            chip(index)
                        }
                    }
                }
                .contentMargins(.horizontal, 16, for: .scrollContent)
                .onChange(of: selected) { _, now in
                    guard let now else { return }
                    withAnimation(reduceMotion ? nil : .smooth) { proxy.scrollTo(now, anchor: .center) }
                }
                .onAppear {
                    if let selected { proxy.scrollTo(selected, anchor: .center) }
                }
            }
            Button(action: add) {
                Label("equaliser.band.add", systemImage: "plus")
                    .font(.subheadline.weight(.semibold))
                    .padding(.horizontal, 14)
                    .frame(minHeight: 44)
                    .contentShape(.capsule)
                    .glassEffect(.regular.interactive(), in: .capsule)
            }
            .buttonStyle(.plain)
            .disabled(full)
        }
    }

    private func chip(_ index: Int) -> some View {
        let isChosen = index == selected
        return Button {
            choose(index)
        } label: {
            Text("\(index + 1)")
                .font(.subheadline.weight(.semibold).monospacedDigit())
                .frame(minWidth: 44, minHeight: 44)
                .foregroundStyle(isChosen ? Color.wornInk : Color.primary)
                .background(isChosen ? Color.worn : Color.primary.opacity(0.08), in: .capsule)
                .contentShape(.capsule)
        }
        .buttonStyle(.plain)
        .id(index)
        .accessibilityLabel(said("equaliser.band.number", index + 1))
        .accessibilityAddTraits(isChosen ? .isSelected : [])
    }
}

struct BandCard: View {
    let band: EqualiserBand
    let number: Int
    let limits: EqualiserLimits
    let moving: (EqualiserBand) -> Void
    let change: (EqualiserBand) -> Void
    let remove: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            Menu {
                Picker("equaliser.band.filter-type", selection: Binding(get: { band.filter }, set: { filter in
                    var next = band
                    next.filter = filter
                    change(next)
                })) {
                    ForEach(EqualiserFilter.every, id: \.self) { filter in
                        Text(filter.label).tag(filter)
                    }
                }
            } label: {
                HStack(spacing: 4) {
                    Text(said("equaliser.band.title", number, band.filter.label))
                        .font(.headline)
                        .foregroundStyle(Color.primary)
                    Image(systemName: "chevron.down")
                        .font(.caption.weight(.bold))
                        .foregroundStyle(Color.worn)
                        .accessibilityHidden(true)
                }
                .frame(minHeight: 44)
                .contentShape(.rect)
            }
            .accessibilityLabel(said("equaliser.band.spoken", number, band.filter.label))
            .accessibilityHint("equaliser.band.filter-type.hint")
            Spacer(minLength: 0)
            Button("common.remove", role: .destructive, action: remove)
                .buttonStyle(.borderless)
                .foregroundStyle(Color.failure)
        }
        MeasuredSlider(
            axis: .gain(around: band.gainDb, limits: limits),
            value: band.gainDb,
            band: number,
            unused: band.filter.usesGain ? nil : band.filter.gainUnused,
            moving: { moving(with(gain: $0)) },
            commit: { change(with(gain: $0)) }
        )
        MeasuredSlider(
            axis: .frequency(limits),
            value: band.frequencyHz,
            band: number,
            moving: { moving(with(hertz: $0)) },
            commit: { change(with(hertz: $0)) }
        )
        MeasuredSlider(
            axis: .width(limits),
            value: band.q,
            band: number,
            moving: { moving(with(q: $0)) },
            commit: { change(with(q: $0)) }
        )
    }

    private func with(gain: Float) -> EqualiserBand {
        var next = band
        next.gainDb = gain
        return next
    }

    private func with(hertz: Float) -> EqualiserBand {
        var next = band
        next.frequencyHz = hertz
        return next
    }

    private func with(q: Float) -> EqualiserBand {
        var next = band
        next.q = q
        return next
    }
}

struct MeasuredSlider<Accessory: View>: View {
    private static var settle: Duration { .milliseconds(400) }

    let axis: BandAxis
    let value: Float
    var band: Int?
    var unused: String?
    let moving: (Float) -> Void
    let commit: (Float) -> Void
    @ViewBuilder let accessory: Accessory

    @State private var dragged: Double?
    @State private var frozen: BandAxis?
    @State private var sliding = false
    @State private var pace = TellingPace()
    @State private var telling: Task<Void, Never>?
    @State private var settling: Task<Void, Never>?
    @State private var entering = false
    @State private var typed = ""

    var body: some View {
        let held = frozen ?? axis
        let shown = dragged.map(held.value(at:)) ?? value
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 8) {
                Text(axis.measure.title)
                Spacer(minLength: 8)
                if let unused {
                    Text(unused)
                        .foregroundStyle(.secondary)
                        .frame(minHeight: 44)
                } else {
                    Button {
                        typed = axis.typing(value)
                        entering = true
                    } label: {
                        Text(axis.written(shown))
                            .monospacedDigit()
                            .foregroundStyle(Color.worn)
                            .frame(minHeight: 44)
                            .contentShape(.rect)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("\(axis.measure.title), \(axis.spoken(shown))")
                    .accessibilityHint("equaliser.value.type-hint")
                }
                accessory
            }
            Slider(value: Binding(get: { dragged ?? held.position(of: value) }, set: slid), in: 0...1) { active in
                sliding = active
                settling?.cancel()
                if active {
                    frozen = axis
                } else {
                    letGo()
                }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(axis.measure.title)
            .accessibilityValue(unused ?? axis.spoken(shown))
            .accessibilityAdjustableAction { direction in
                commit(axis.stepped(value, up: direction == .increment))
            }
            .disabled(unused != nil)
        }
        .alert(axis.entering(band: band), isPresented: $entering) {
            TextField(axis.measure.title, text: $typed)
                .keyboardType(.numbersAndPunctuation)
            Button("equaliser.value.set", action: take)
            Button("common.cancel", role: .cancel) {}
        } message: {
            Text(axis.range)
        }
    }

    private func slid(_ position: Double) {
        dragged = position
        let chosen = (frozen ?? axis).value(at: position)
        settling?.cancel()
        if !sliding {
            settling = Task {
                try? await Task.sleep(for: Self.settle)
                guard !Task.isCancelled, !sliding else { return }
                letGo()
            }
        }
        guard let wait = pace.ask(at: .now) else {
            moving(chosen)
            return
        }
        guard telling == nil else { return }
        telling = Task {
            try? await Task.sleep(for: wait)
            telling = nil
            guard !Task.isCancelled, let dragged else { return }
            pace.told(at: .now)
            moving((frozen ?? axis).value(at: dragged))
        }
    }

    private func letGo() {
        let chosen = dragged.map((frozen ?? axis).value(at:))
        telling?.cancel()
        telling = nil
        pace.forget()
        dragged = nil
        frozen = nil
        if let chosen { commit(chosen) }
    }

    private func take() {
        guard let read = axis.read(typed), read != value else { return }
        commit(read)
    }
}

extension MeasuredSlider where Accessory == EmptyView {
    init(
        axis: BandAxis,
        value: Float,
        band: Int? = nil,
        unused: String? = nil,
        moving: @escaping (Float) -> Void,
        commit: @escaping (Float) -> Void
    ) {
        self.init(axis: axis, value: value, band: band, unused: unused, moving: moving, commit: commit) { EmptyView() }
    }
}

private struct PreampSection: View {
    let curve: EqualiserCurve
    let limits: EqualiserLimits
    let moving: (Float) -> Void
    let change: (Float) -> Void
    let fit: () -> Void

    var body: some View {
        Section {
            MeasuredSlider(
                axis: .preamp(around: curve.preampDb, limits: limits),
                value: curve.preampDb,
                moving: moving,
                commit: change
            ) {
                Button("equaliser.preamp.fit", action: fit)
                    .font(.subheadline.weight(.semibold))
                    .buttonStyle(.bordered)
                    .buttonBorderShape(.capsule)
                    .disabled(curve.bands.isEmpty)
                    .accessibilityLabel("equaliser.preamp.fit.spoken")
            }
        } header: {
            Text("equaliser.preamp")
        } footer: {
            Text("equaliser.preamp.footer")
        }
    }
}

private struct PresetsSection: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let presets: [EqualiserPreset]
    let chosen: String
    let choose: (EqualiserPreset) -> Void

    var body: some View {
        let preset = presets.first { $0.name == chosen }
        Section {
            ScrollViewReader { proxy in
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(Array(presets.enumerated()), id: \.element.name) { place, preset in
                            chip(preset, mark: EqualiserPreset.mark(at: place))
                        }
                    }
                }
                .contentMargins(.horizontal, 16, for: .scrollContent)
                .onAppear {
                    if let preset { proxy.scrollTo(preset.name, anchor: .center) }
                }
            }
            .listRowInsets(EdgeInsets(top: 8, leading: 0, bottom: 8, trailing: 0))
        } header: {
            Text("equaliser.presets")
        } footer: {
            Text(preset?.detail ?? said("equaliser.presets.own"))
        }
    }

    private func chip(_ preset: EqualiserPreset, mark: String) -> some View {
        let isChosen = preset.name == chosen
        return Button {
            choose(preset)
        } label: {
            Label(preset.name, systemImage: mark)
                .font(.subheadline.weight(.medium))
                .padding(.horizontal, 14)
                .frame(minHeight: 44)
                .foregroundStyle(isChosen ? Color.wornInk : Color.primary)
                .background(isChosen ? Color.worn : Color.primary.opacity(0.08), in: .capsule)
                .contentShape(.capsule)
        }
        .buttonStyle(.plain)
        .id(preset.name)
        .accessibilityHint(preset.detail)
        .accessibilityAddTraits(isChosen ? .isSelected : [])
    }
}

private struct CorrectionsSection: View {
    @Environment(Library.self) private var library

    var body: some View {
        Section("equaliser.corrections") {
            NavigationLink(value: Destination.headphoneCorrection) {
                ToolRow(
                    title: SettingsPlace.headphoneCorrection.title,
                    detail: SettingsPlace.headphoneCorrection.summary,
                    symbol: SettingsPlace.headphoneCorrection.symbol
                )
            }
            NavigationLink(value: Destination.microphoneMeasurement) {
                ToolRow(
                    title: SettingsPlace.microphoneMeasurement.title,
                    detail: SettingsPlace.microphoneMeasurement.summary,
                    symbol: SettingsPlace.microphoneMeasurement.symbol
                )
            }
            NavigationLink(value: Destination.roomCorrection) {
                RoomCorrectionRow()
            }
            NavigationLink(value: Destination.listeningTest) {
                ToolRow(
                    title: SettingsPlace.listeningTest.title,
                    detail: SettingsPlace.listeningTest.summary,
                    symbol: SettingsPlace.listeningTest.symbol
                )
            }
            let others = library.boundRoutes.filter { $0.id != library.route?.id }
            if !others.isEmpty {
                NavigationLink(value: Destination.otherOutputs) {
                    ToolRow(
                        title: SettingsPlace.otherOutputs.title,
                        detail: others.map(\.name).joined(separator: " · "),
                        symbol: SettingsPlace.otherOutputs.symbol
                    )
                }
            }
        }
    }
}

struct RoomCorrectionRow: View {
    @Environment(Library.self) private var library

    var body: some View {
        ToolRow(
            title: SettingsPlace.roomCorrection.title,
            detail: Self.status(of: library.features.room),
            symbol: SettingsPlace.roomCorrection.symbol
        )
    }

    static func status(of room: RoomCorrectionState) -> String {
        guard let name = room.name else { return SettingsPlace.roomCorrection.summary }
        return said(room.on ? "equaliser.room.on" : "equaliser.room.off", name)
    }
}

enum PendingProfile {
    case device(AutoEqDevice)
    case text(name: String, bytes: Data)

    var name: String {
        switch self {
        case .device(let device): device.name
        case .text(let name, _): name
        }
    }
}

struct HeadphoneCorrectionView: View {
    private static let searchSettles: Duration = .milliseconds(350)
    private static let largestProfile = 64 * 1024

    @Environment(Library.self) private var library
    @State private var model = ""
    @State private var pending: PendingProfile?
    @State private var choosingFile = false

    var body: some View {
        let features = library.features
        let current = library.preferences?.equaliser.name ?? ""
        let found = HeadphoneMatches.shown(features.corrections)
        ThemedForm {
            Section {
                HStack(spacing: 10) {
                    Image(systemName: "magnifyingglass")
                        .foregroundStyle(.secondary)
                        .accessibilityHidden(true)
                    TextField("headphone-correction.search", text: $model)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .submitLabel(.search)
                        .onSubmit { search(model) }
                }
                if !features.online {
                    Text("headphone-correction.offline").foregroundStyle(.secondary)
                } else if features.searchingCorrections {
                    HStack(spacing: 12) {
                        ProgressView().accessibilityHidden(true)
                        Text("headphone-correction.searching").foregroundStyle(.secondary)
                    }
                } else if let searched = features.correctionsFor, features.corrections.isEmpty {
                    Text(said("headphone-correction.none", searched)).foregroundStyle(.secondary)
                }
                ForEach(found.shown, id: \.id) { device in
                    Button {
                        ask(.device(device))
                    } label: {
                        HStack {
                            Label(device.name, systemImage: "headphones").foregroundStyle(Color.primary)
                            Spacer()
                            if current == device.name {
                                Image(systemName: "checkmark")
                                    .foregroundStyle(.tint)
                                    .accessibilityLabel("headphone-correction.in-use")
                            }
                        }
                    }
                    .disabled(features.applyingCorrection)
                }
                if found.more > 0 {
                    Text(said("headphone-correction.more", found.more))
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            } header: {
                Text("headphone-correction.autoeq")
            } footer: {
                Text("headphone-correction.autoeq.footer")
            }
            Section {
                Button { paste() } label: {
                    ToolRow(title: said("headphone-correction.paste"), detail: said("headphone-correction.paste.detail"), symbol: "doc.on.clipboard")
                }
                Button { choosingFile = true } label: {
                    ToolRow(title: said("headphone-correction.import-file"), detail: said("headphone-correction.import-file.detail"), symbol: "doc.text")
                }
            } header: {
                Text("headphone-correction.import")
            } footer: {
                Text("headphone-correction.import.footer")
            }
        }
        .navigationTitle("headphone-correction.title")
        .task(id: model) {
            let text = model.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !text.isEmpty else {
                library.features.clearCorrections()
                return
            }
            try? await Task.sleep(for: Self.searchSettles)
            guard !Task.isCancelled else { return }
            search(text)
        }
        .confirmationDialog(
            "headphone-correction.replace.question",
            isPresented: Binding(get: { pending != nil }, set: { if !$0 { pending = nil } }),
            titleVisibility: .visible,
            presenting: pending
        ) { chosen in
            Button(said("headphone-correction.load", chosen.name)) { load(chosen) }
        } message: { chosen in
            Text(said("headphone-correction.replace.message", library.preferences?.equaliser.bands.count ?? 0, chosen.name))
        }
        .fileImporter(isPresented: $choosingFile, allowedContentTypes: [.plainText, .text, .data]) { result in
            switch result {
            case .success(let url): read(url)
            case .failure(let failure): library.report(failure)
            }
        }
    }

    private func search(_ text: String) {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard library.features.online, !text.isEmpty else { return }
        library.features.searchCorrections(text)
    }

    private func ask(_ chosen: PendingProfile) {
        if library.preferences?.equaliser.bands.isEmpty ?? true {
            load(chosen)
        } else {
            pending = chosen
        }
    }

    private func load(_ chosen: PendingProfile) {
        pending = nil
        switch chosen {
        case .device(let device): library.features.applyCorrection(device)
        case .text(let name, let bytes): library.features.applyProfile(name: name, bytes: bytes)
        }
    }

    private func paste() {
        guard let text = UIPasteboard.general.string, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            library.notice(said("headphone-correction.clipboard-empty"))
            return
        }
        ask(.text(name: said("headphone-correction.pasted-name"), bytes: Data(text.utf8)))
    }

    private func read(_ url: URL) {
        let name = url.deletingPathExtension().lastPathComponent
        let largest = Self.largestProfile
        Task {
            do {
                let bytes = try await Work.aside { try Self.contents(of: url, atMost: largest) }
                ask(.text(name: name, bytes: bytes))
            } catch {
                library.report(error)
            }
        }
    }

    nonisolated private static func contents(of url: URL, atMost largest: Int) throws -> Data {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        return try handle.read(upToCount: largest + 1) ?? Data()
    }
}

struct OtherOutputsView: View {
    @Environment(Library.self) private var library

    var body: some View {
        let others = library.boundRoutes.filter { $0.id != library.route?.id }
        ThemedForm {
            Section {
                ForEach(others, id: \.id) { route in
                    Label(route.name, systemImage: "hifispeaker")
                        .swipeActions {
                            Button("other-outputs.forget", role: .destructive) { library.forgetEqualiser(for: route) }
                                .tint(.red)
                        }
                        .contextMenu {
                            Button("other-outputs.forget-curve", systemImage: "trash", role: .destructive) {
                                library.forgetEqualiser(for: route)
                            }
                        }
                }
            } footer: {
                Text(said(others.isEmpty ? "other-outputs.none" : "other-outputs.footer"))
            }
        }
        .navigationTitle("other-outputs.title")
    }
}
