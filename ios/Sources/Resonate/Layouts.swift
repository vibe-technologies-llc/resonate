import ResonateLogic
import SwiftUI

extension SizeClass {
    init(_ sizeClass: UserInterfaceSizeClass?) {
        self = sizeClass == .regular ? .regular : .compact
    }
}

struct ShapedScreen<Content: View>: View {
    @Environment(\.horizontalSizeClass) private var horizontal
    @Environment(\.verticalSizeClass) private var vertical
    @Environment(\.dynamicTypeSize) private var typeSize
    @ViewBuilder let content: (ScreenShape) -> Content

    var body: some View {
        GeometryReader { space in
            content(
                ScreenShape(
                    horizontal: SizeClass(horizontal),
                    vertical: SizeClass(vertical),
                    width: space.size.width,
                    height: space.size.height,
                    accessibilityText: typeSize.isAccessibilitySize,
                    fold: Fold.within(space)
                )
            )
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

extension Fold {
    static func within(_ space: GeometryProxy) -> Fold? {
        guard #available(iOS 27.1, *), let division = space.reservedRegions(kind: .division).first else { return nil }
        let frame = division.frame
        return frame.height > frame.width
            ? Fold(runs: .down, start: frame.minX, end: frame.maxX)
            : Fold(runs: .across, start: frame.minY, end: frame.maxY)
    }
}

struct SheetSized: ViewModifier {
    @Environment(\.dynamicTypeSize) private var typeSize
    let weight: SheetWeight

    func body(content: Content) -> some View {
        switch SheetSizing.of(weight, accessibilityText: typeSize.isAccessibilitySize) {
        case .form: content.presentationSizing(.form)
        case .page: content.presentationSizing(.page)
        }
    }
}

extension View {
    func sheetSized(_ weight: SheetWeight = .short) -> some View {
        modifier(SheetSized(weight: weight))
    }
}

extension LibrarySection {
    var destination: Destination? {
        switch self {
        case .home: nil
        case .favourites: .favourites
        case .songs: .songs
        case .playlists: .playlists
        case .albums: .albums
        case .artists: .artists
        }
    }
}

struct LibrarySidebar: View {
    @Environment(Navigation.self) private var navigation

    var body: some View {
        let selection = Binding<LibrarySection?>(
            get: { navigation.librarySection },
            set: { if let section = $0 { navigation.choose(section) } }
        )
        List(LibrarySection.allCases, selection: selection) { section in
            Label(section.title, systemImage: section.symbol)
                .tag(section)
        }
        .listStyle(.sidebar)
        .navigationTitle("layouts.library")
    }
}

struct LibrarySplit<Detail: View>: View {
    @Environment(Navigation.self) private var navigation
    @ViewBuilder let detail: (LibrarySection) -> Detail

    var body: some View {
        NavigationSplitView {
            LibrarySidebar()
        } detail: {
            detail(navigation.librarySection)
        }
    }
}

struct LibrarySectionRoot: View {
    let section: LibrarySection

    var body: some View {
        if let destination = section.destination {
            DestinationView(destination: destination)
        } else {
            LibraryView()
        }
    }
}

struct GraphFrame: Layout {
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.replacingUnspecifiedDimensions().width
        return CGSize(width: width, height: EqualiserGraphShape.height(forWidth: Double(width)))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for subview in subviews {
            subview.place(at: bounds.origin, proposal: ProposedViewSize(bounds.size))
        }
    }
}

struct Shelf<Cards: View>: View {
    private static var gutter: CGFloat { 16 }

    @Environment(\.dynamicTypeSize) private var typeSize
    private let spacing: CGFloat
    private let cards: Cards

    init(spacing: CGFloat, @ViewBuilder cards: () -> Cards) {
        self.spacing = spacing
        self.cards = cards()
    }

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            if typeSize.isAccessibilitySize {
                HStack(alignment: .top, spacing: spacing) { cards }
                    .padding(.horizontal, Self.gutter)
            } else {
                LazyHStack(alignment: .top, spacing: spacing) { cards }
                    .padding(.horizontal, Self.gutter)
            }
        }
    }
}
