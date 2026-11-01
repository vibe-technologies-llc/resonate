import Testing

@testable import ResonateLogic

@Suite struct LayoutsTests {
    private static let iPhonePortrait = shape(.compact, .regular, 393, 852)
    private static let iPhoneLandscape = shape(.compact, .compact, 852, 393)
    private static let iPhoneMaxLandscape = shape(.regular, .compact, 932, 430)
    private static let iPadPortrait = shape(.regular, .regular, 820, 1180)
    private static let iPadLandscape = shape(.regular, .regular, 1180, 820)
    private static let iPadSlideOver = shape(.compact, .regular, 320, 1000)
    private static let iPadSplitView = shape(.compact, .regular, 507, 820)

    private static func shape(_ horizontal: SizeClass, _ vertical: SizeClass, _ width: Double, _ height: Double) -> ScreenShape {
        ScreenShape(horizontal: horizontal, vertical: vertical, width: width, height: height, accessibilityText: false)
    }

    private static func large(_ shape: ScreenShape) -> ScreenShape {
        ScreenShape(
            horizontal: shape.horizontal,
            vertical: shape.vertical,
            width: shape.width,
            height: shape.height,
            accessibilityText: true
        )
    }

    @Test func thePlayerStaysOneColumnOnAPhoneUpright() {
        let arrangement = PlayerArrangement.of(Self.iPhonePortrait)

        #expect(!arrangement.sideBySide)
        #expect(!arrangement.scrolls)
    }

    @Test func thePlayerSetsTheCoverBesideTheControlsOnAPhoneOnItsSide() {
        for shape in [Self.iPhoneLandscape, Self.iPhoneMaxLandscape] {
            let arrangement = PlayerArrangement.of(shape)

            #expect(arrangement.sideBySide)
            #expect(arrangement.scrolls)
        }
    }

    @Test func thePlayerSetsTheCoverBesideTheControlsOnAnIPadOnItsSideAndDoesNotScroll() {
        let arrangement = PlayerArrangement.of(Self.iPadLandscape)

        #expect(arrangement.sideBySide)
        #expect(!arrangement.scrolls)
    }

    @Test func thePlayerStaysOneColumnOnAnIPadUprightAndInANarrowWindow() {
        for shape in [Self.iPadPortrait, Self.iPadSlideOver, Self.iPadSplitView] {
            let arrangement = PlayerArrangement.of(shape)

            #expect(!arrangement.sideBySide)
            #expect(!arrangement.scrolls)
        }
    }

    @Test func accessibilityTextScrollsEverywhereAndKeepsAPhoneOnItsSideInOneColumn() {
        for shape in [Self.iPhonePortrait, Self.iPhoneLandscape, Self.iPhoneMaxLandscape, Self.iPadPortrait, Self.iPadSlideOver] {
            let arrangement = PlayerArrangement.of(Self.large(shape))

            #expect(!arrangement.sideBySide)
            #expect(arrangement.scrolls)
        }
    }

    @Test func accessibilityTextOnAnIPadOnItsSideKeepsTwoColumnsAndScrolls() {
        let arrangement = PlayerArrangement.of(Self.large(Self.iPadLandscape))

        #expect(arrangement.sideBySide)
        #expect(arrangement.scrolls)
    }

    @Test func aSquareWindowIsNotWiderThanTall() {
        let square = Self.shape(.regular, .regular, 800, 800)

        #expect(!square.isWiderThanTall)
        #expect(!PlayerArrangement.of(square).sideBySide)
    }

    @Test func theLibraryIsASplitOnlyWhereTheWidthIsRegularAndTheTextIsNotLarge() {
        #expect(LibraryArrangement.of(horizontal: Self.iPadPortrait.horizontal, accessibilityText: false) == .split)
        #expect(LibraryArrangement.of(horizontal: Self.iPadLandscape.horizontal, accessibilityText: false) == .split)
        #expect(LibraryArrangement.of(horizontal: Self.iPhoneMaxLandscape.horizontal, accessibilityText: false) == .split)
        #expect(LibraryArrangement.of(horizontal: Self.iPhonePortrait.horizontal, accessibilityText: false) == .stack)
        #expect(LibraryArrangement.of(horizontal: Self.iPhoneLandscape.horizontal, accessibilityText: false) == .stack)
        #expect(LibraryArrangement.of(horizontal: Self.iPadSlideOver.horizontal, accessibilityText: false) == .stack)
        #expect(LibraryArrangement.of(horizontal: Self.iPadSplitView.horizontal, accessibilityText: false) == .stack)
    }

    @Test func theLibraryFallsBackToTheStackAtAccessibilityTextSizes() {
        #expect(LibraryArrangement.of(horizontal: .regular, accessibilityText: true) == .stack)
        #expect(LibraryArrangement.of(horizontal: .compact, accessibilityText: true) == .stack)
    }

    @Test func theSidebarListsEverySectionOnceStartingAtHome() {
        let every = LibrarySection.allCases

        #expect(every.first == .home)
        #expect(every.filter(\.isHome).count == 1)
        #expect(Set(every.map(\.id)).count == every.count)
        #expect(Set(every.map(\.title)).count == every.count)
        #expect(every.allSatisfy { !$0.symbol.isEmpty })
    }

    @Test func aShortSheetIsAFormAndALongOneAPage() {
        #expect(SheetSizing.of(.short, accessibilityText: false) == .form)
        #expect(SheetSizing.of(.long, accessibilityText: false) == .page)
    }

    @Test func everySheetIsAPageAtAccessibilityTextSizes() {
        #expect(SheetSizing.of(.short, accessibilityText: true) == .page)
        #expect(SheetSizing.of(.long, accessibilityText: true) == .page)
    }
}
