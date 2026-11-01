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
                    accessibilityText: typeSize.isAccessibilitySize
                )
            )
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
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
        case .suggestions: .suggestions
        case .statistics: .statistics
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
        .navigationTitle("Library")
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
