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
    private static let duoClosed = shape(.compact, .regular, 466, 678)
    private static let duoClosedOnItsSide = shape(.compact, .compact, 678, 466)
    private static let duoOpen = shape(.regular, .regular, 669, 951)
    private static let duoOpenOnItsSide = shape(.regular, .regular, 951, 669)
    private static let duoAsABook = folded(duoOpenOnItsSide, Fold(runs: .down, start: 459, end: 492))
    private static let duoOnATable = folded(duoOpen, Fold(runs: .across, start: 459, end: 492))

    private static func shape(_ horizontal: SizeClass, _ vertical: SizeClass, _ width: Double, _ height: Double) -> ScreenShape {
        ScreenShape(horizontal: horizontal, vertical: vertical, width: width, height: height, accessibilityText: false)
    }

    private static func large(_ shape: ScreenShape) -> ScreenShape {
        ScreenShape(
            horizontal: shape.horizontal,
            vertical: shape.vertical,
            width: shape.width,
            height: shape.height,
            accessibilityText: true,
            fold: shape.fold
        )
    }

    private static func folded(_ shape: ScreenShape, _ fold: Fold) -> ScreenShape {
        ScreenShape(
            horizontal: shape.horizontal,
            vertical: shape.vertical,
            width: shape.width,
            height: shape.height,
            accessibilityText: shape.accessibilityText,
            fold: fold
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

    @Test func aDuoClosedIsAPhoneAndOpenFlatIsAnIPad() {
        #expect(PlayerArrangement.of(Self.duoClosed) == PlayerArrangement(sideBySide: false, scrolls: false))
        #expect(PlayerArrangement.of(Self.duoClosedOnItsSide) == PlayerArrangement(sideBySide: true, scrolls: true))
        #expect(PlayerArrangement.of(Self.duoOpen) == PlayerArrangement(sideBySide: false, scrolls: false))
        #expect(PlayerArrangement.of(Self.duoOpenOnItsSide) == PlayerArrangement(sideBySide: true, scrolls: false))
    }

    @Test func aDuoHeldAsABookMeetsItsColumnsAtTheFold() {
        let arrangement = PlayerArrangement.of(Self.duoAsABook)

        #expect(arrangement.sideBySide)
        #expect(!arrangement.scrolls)
        #expect(!arrangement.stackedAtTheFold)
        #expect(arrangement.seam == .beside(leading: 459, gap: 33, trailing: 459))
    }

    @Test func aDuoOnATableShowsTheCoverAboveTheFoldAndTheControlsBelow() {
        let arrangement = PlayerArrangement.of(Self.duoOnATable)

        #expect(!arrangement.sideBySide)
        #expect(!arrangement.scrolls)
        #expect(arrangement.stackedAtTheFold)
        #expect(arrangement.seam == .stacked(top: 459, gap: 33, bottom: 459))
    }

    @Test func accessibilityTextAtTheFoldKeepsTheSeamAndScrolls() {
        for shape in [Self.duoAsABook, Self.duoOnATable] {
            let arrangement = PlayerArrangement.of(Self.large(shape))

            #expect(arrangement.seam == PlayerArrangement.of(shape).seam)
            #expect(arrangement.scrolls)
        }
    }

    @Test func aFoldLeavingTooNarrowASideIsIgnored() {
        let nearTheEdge = Self.folded(Self.duoOpenOnItsSide, Fold(runs: .down, start: 120, end: 150))
        let empty = Self.folded(Self.duoOpenOnItsSide, Fold(runs: .down, start: 475, end: 475))

        #expect(PlayerArrangement.of(nearTheEdge) == PlayerArrangement.of(Self.duoOpenOnItsSide))
        #expect(PlayerArrangement.of(empty) == PlayerArrangement.of(Self.duoOpenOnItsSide))
    }

    @Test func theLibraryIsASplitOnlyWhereTheWidthIsRegularAndTheTextIsNotLarge() {
        #expect(LibraryArrangement.of(horizontal: Self.iPadPortrait.horizontal, accessibilityText: false) == .split)
        #expect(LibraryArrangement.of(horizontal: Self.iPadLandscape.horizontal, accessibilityText: false) == .split)
        #expect(LibraryArrangement.of(horizontal: Self.iPhoneMaxLandscape.horizontal, accessibilityText: false) == .split)
        #expect(LibraryArrangement.of(horizontal: Self.iPhonePortrait.horizontal, accessibilityText: false) == .stack)
        #expect(LibraryArrangement.of(horizontal: Self.iPhoneLandscape.horizontal, accessibilityText: false) == .stack)
        #expect(LibraryArrangement.of(horizontal: Self.iPadSlideOver.horizontal, accessibilityText: false) == .stack)
        #expect(LibraryArrangement.of(horizontal: Self.iPadSplitView.horizontal, accessibilityText: false) == .stack)
        #expect(LibraryArrangement.of(horizontal: Self.duoOpen.horizontal, accessibilityText: false) == .split)
        #expect(LibraryArrangement.of(horizontal: Self.duoClosed.horizontal, accessibilityText: false) == .stack)
    }

    @Test func theLibraryFallsBackToTheStackAtAccessibilityTextSizes() {
        #expect(LibraryArrangement.of(horizontal: .regular, accessibilityText: true) == .stack)
        #expect(LibraryArrangement.of(horizontal: .compact, accessibilityText: true) == .stack)
    }

    @Test func theListeningGlanceStandsBesideItsChartOnlyWhereTheWidthIsRegularAndTheTextIsNotLarge() {
        #expect(ListeningGlanceArrangement.of(horizontal: Self.iPadLandscape.horizontal, accessibilityText: false) == .beside)
        #expect(ListeningGlanceArrangement.of(horizontal: Self.duoOpen.horizontal, accessibilityText: false) == .beside)
        #expect(ListeningGlanceArrangement.of(horizontal: Self.iPhonePortrait.horizontal, accessibilityText: false) == .stacked)
        #expect(ListeningGlanceArrangement.of(horizontal: Self.iPhoneLandscape.horizontal, accessibilityText: false) == .stacked)
        #expect(ListeningGlanceArrangement.of(horizontal: .regular, accessibilityText: true) == .stacked)
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
