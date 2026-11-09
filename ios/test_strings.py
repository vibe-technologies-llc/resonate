import plistlib
import unittest

import words

PROPER = {"Resonate", "Monochrome", "Subsonic", "Jellyfin", "TIDAL", "Spotify", "AirPlay"}

STILL_TO_MOVE = {
    "Resonate/Appearance.swift",
    "Resonate/Artwork.swift",
    "Resonate/Backdrop.swift",
    "Resonate/CollectionViews.swift",
    "Resonate/Deleting.swift",
    "Resonate/DownloadQueue.swift",
    "Resonate/DownloadViews.swift",
    "Resonate/DrivenDeviceView.swift",
    "Resonate/Enrichment.swift",
    "Resonate/EqualiserGraph.swift",
    "Resonate/EqualiserView.swift",
    "Resonate/Features.swift",
    "Resonate/Folders.swift",
    "Resonate/HomeViews.swift",
    "Resonate/Layouts.swift",
    "Resonate/Library.swift",
    "Resonate/LibraryToolViews.swift",
    "Resonate/ListenViews.swift",
    "Resonate/Listening.swift",
    "Resonate/ListeningGlanceViews.swift",
    "Resonate/ListeningTest.swift",
    "Resonate/LyricsPane.swift",
    "Resonate/MeasuredCorrection.swift",
    "Resonate/NowPlaying.swift",
    "Resonate/OnlineViews.swift",
    "Resonate/OrganiseView.swift",
    "Resonate/PlayerViews.swift",
    "Resonate/PlaylistViews.swift",
    "Resonate/ResonateApp.swift",
    "Resonate/RoomCorrection.swift",
    "Resonate/SavedSearchSheet.swift",
    "Resonate/SearchModel.swift",
    "Resonate/SearchTab.swift",
    "Resonate/SearchViews.swift",
    "Resonate/ServerViews.swift",
    "Resonate/SettingsViews.swift",
    "Resonate/Sharing.swift",
    "Resonate/Siri.swift",
    "Resonate/SongViews.swift",
    "Resonate/SpotifyView.swift",
    "Resonate/StatisticsView.swift",
    "Resonate/SyncView.swift",
    "Resonate/TagEditor.swift",
    "Resonate/TogetherViews.swift",
    "Resonate/ToolViews.swift",
    "Resonate/WrappedViews.swift",
    "Resonate/ZonesSheet.swift",
    "ResonateLogic/BackdropShader.swift",
    "ResonateLogic/ConnectionMeters.swift",
    "ResonateLogic/Equaliser.swift",
    "ResonateLogic/EqualiserEditing.swift",
    "ResonateLogic/Erasure.swift",
    "ResonateLogic/Home.swift",
    "ResonateLogic/Keychain.swift",
    "ResonateLogic/Layouts.swift",
    "ResonateLogic/LinkedPlaylists.swift",
    "ResonateLogic/ListeningBar.swift",
    "ResonateLogic/ListeningGlance.swift",
    "ResonateLogic/ListeningTestWords.swift",
    "ResonateLogic/Lists.swift",
    "ResonateLogic/Lyrics.swift",
    "ResonateLogic/Measurement.swift",
    "ResonateLogic/MicrophoneFailure.swift",
    "ResonateLogic/Names+Hits.swift",
    "ResonateLogic/Online.swift",
    "ResonateLogic/RoomPair.swift",
    "ResonateLogic/SavedSearchDraft.swift",
    "ResonateLogic/Served.swift",
    "ResonateLogic/SettingsCatalog.swift",
    "ResonateLogic/SpotifyWords.swift",
    "ResonateLogic/Suggestions.swift",
    "ResonateLogic/Sync.swift",
    "ResonateLogic/TagDiff.swift",
    "ResonateLogic/Tally.swift",
    "ResonateLogic/Together.swift",
    "ResonateLogic/Wrapped.swift",
    "ResonateLogic/Zones.swift",
}


def prose_by_file():
    found = {}
    for path in words.swift_files():
        prose = [literal for literal in words.prose_of(path.read_text(encoding="utf-8")) if literal.text not in PROPER]
        if prose:
            found[path.relative_to(words.sources).as_posix()] = prose
    return found


class Strings(unittest.TestCase):
    def test_every_word_the_app_shows_is_said_through_its_strings(self):
        found = prose_by_file()

        unlisted = {path: prose for path, prose in found.items() if path not in STILL_TO_MOVE}
        moved = sorted(STILL_TO_MOVE - found.keys())

        self.assertEqual(
            unlisted,
            {},
            "\n".join(f'{path}:{literal.line}: "{literal.text}"' for path, prose in unlisted.items() for literal in prose),
        )
        self.assertEqual(moved, [], "moved already, so no longer still to move")

    def test_every_key_the_swift_names_is_in_the_english_strings(self):
        known = words.strings_file(words.english / "Localizable.strings").keys() | words.plurals().keys()
        used = set()
        for path in words.swift_files():
            used |= words.keys_used_in(path.read_text(encoding="utf-8"))

        self.assertEqual(sorted(used - known), [])

    def test_the_prompts_the_system_shows_are_said_in_english_as_the_info_plist_says_them(self):
        with (words.ios / "Resonate/Info.plist").open("rb") as plist:
            info = plistlib.load(plist)
        prompts = words.strings_file(words.english / "InfoPlist.strings")

        self.assertEqual(info["CFBundleDevelopmentRegion"], "en")
        self.assertIn("en", info["CFBundleLocalizations"])
        for key, said in prompts.items():
            self.assertEqual(said, info[key], key)
        self.assertLessEqual({key for key in info if key.endswith("UsageDescription")}, prompts.keys())

    def test_the_english_strings_land_in_the_bundle(self):
        self.assertIn("  - Resonate/en.lproj\n", (words.ios / "xtool.yml").read_text())


class Prose(unittest.TestCase):
    def test_prose_is_text_a_listener_reads_and_not_a_key_or_a_log_line(self):
        source = '\n'.join(
            [
                'let key = "download-wants-automatically"',
                'Self.log.error("The backdrop failed: \\(error)")',
                'precondition(count > 0, "A count must be above zero")',
                'Text("Play next")',
                'let shown = "\\(count) songs"',
                'Image(systemName: "play.fill")',
            ]
        )

        self.assertEqual([literal.text for literal in words.prose_of(source)], ["Play next", "{} songs"])

    def test_a_key_is_named_where_the_view_asks_for_its_words(self):
        source = 'Text("transport.play")\nlet tag = "transport.pause"\nString(localized: "queue.clear")'

        self.assertEqual(words.keys_used_in(source), {"transport.play", "queue.clear"})


if __name__ == "__main__":
    unittest.main()
