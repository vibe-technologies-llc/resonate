import Foundation
import ResonateWidgetLinks
import Testing

@Suite struct WidgetLinkTests {
    @Test(arguments: WidgetLink.allCases)
    func eachShortcutRoundTrips(_ link: WidgetLink) {
        #expect(WidgetLink.recognizes(link.url))
        #expect(WidgetLink(url: link.url) == link)
    }

    @Test(arguments: [
        "resonate://widget",
        "resonate://widget/unknown",
        "resonate://widget/shuffle?track=2",
        "resonate://widget/shuffle#track",
        "resonate://listener@widget/shuffle",
    ])
    func unknownWidgetLinksStayOutOfPairing(_ address: String) throws {
        let url = try #require(URL(string: address))
        #expect(WidgetLink.recognizes(url))
        #expect(WidgetLink(url: url) == nil)
    }

    @Test(arguments: [
        "resonate://shared/token",
        "resonate://party/token",
        "resonate://pair/token",
        "file:///Music/song.flac",
        "https://widget/shuffle",
    ])
    func otherLinksKeepTheirOwnRouting(_ address: String) throws {
        let url = try #require(URL(string: address))
        #expect(!WidgetLink.recognizes(url))
        #expect(WidgetLink(url: url) == nil)
    }
}
