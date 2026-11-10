import plistlib
import re
import unittest

import words

PROPER = {"Resonate", "Monochrome", "Subsonic", "Jellyfin", "TIDAL", "Spotify", "AirPlay", "Localizable"}

NOT_WORDS = {"ResonateLogic/BackdropShader.swift"}


def prose_by_file():
    found = {}
    for path in words.swift_files():
        if path.relative_to(words.sources).as_posix() in NOT_WORDS:
            continue
        prose = [literal for literal in words.prose_of(path.read_text(encoding="utf-8")) if literal.text not in PROPER]
        if prose:
            found[path.relative_to(words.sources).as_posix()] = prose
    return found


class Strings(unittest.TestCase):
    def test_every_word_the_app_shows_is_said_through_its_strings(self):
        found = prose_by_file()

        self.assertEqual(
            found,
            {},
            "\n".join(f'{path}:{literal.line}: "{literal.text}"' for path, prose in found.items() for literal in prose),
        )

    def test_a_file_that_holds_no_words_still_holds_text_that_reads_as_words(self):
        for path in NOT_WORDS:
            self.assertTrue(words.prose_of((words.sources / path).read_text(encoding="utf-8")), path)

    def test_every_key_the_swift_names_is_in_the_english_strings(self):
        known = words.strings_file(words.english / "Localizable.strings").keys() | words.plurals().keys()
        used = set()
        for path in words.swift_files():
            used |= words.keys_used_in(path.read_text(encoding="utf-8"))

        self.assertEqual(sorted(used - known), [])

    def test_every_key_in_the_english_strings_is_said_somewhere(self):
        known = words.strings_file(words.english / "Localizable.strings").keys() | words.plurals().keys()
        said = set()
        for path in words.swift_files():
            said |= {literal.text for literal in words.literals_of(path.read_text(encoding="utf-8"))}

        self.assertEqual(sorted(known - said), [])

    def test_a_key_is_named_only_where_it_is_said(self):
        known = words.strings_file(words.english / "Localizable.strings").keys() | words.plurals().keys()
        unsaid = []
        for path in words.swift_files():
            for literal in words.literals_of(path.read_text(encoding="utf-8")):
                if literal.text in known and not literal.keyed:
                    unsaid.append(f"{path.relative_to(words.sources).as_posix()}:{literal.line}: {literal.text}")

        self.assertEqual(unsaid, [])

    def test_every_count_chooses_between_one_and_other(self):
        for key, rule in words.plurals().items():
            variables = set(re.findall(r"%(?:\d+\$)?#@(\w+)@", rule["NSStringLocalizedFormatKey"]))

            self.assertTrue(variables, key)
            self.assertEqual(variables, rule.keys() - {"NSStringLocalizedFormatKey"}, key)
            for variable in variables:
                self.assertEqual(rule[variable]["NSStringFormatSpecTypeKey"], "NSStringPluralRuleType", key)
                self.assertLessEqual({"one", "other"}, rule[variable].keys(), key)

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

    def test_a_key_is_named_by_said_and_by_a_control_but_a_symbol_is_not_a_key(self):
        source = '\n'.join(
            [
                'let shown = said("songs.counted", count)',
                'Button(on ? "favourite.remove" : "favourite.add", systemImage: on ? "heart.slash" : "heart") {}',
                'Label("downloads.title", systemImage: "arrow.down.circle")',
                '.navigationTitle("settings.title")',
            ]
        )

        self.assertEqual(
            words.keys_used_in(source),
            {"songs.counted", "favourite.remove", "favourite.add", "downloads.title", "settings.title"},
        )

    def test_what_an_app_intent_declares_for_the_system_stays_literal(self):
        source = '\n'.join(
            [
                'static let title: LocalizedStringResource = "Play Favourites"',
                'static let description = IntentDescription("Plays the songs you marked.")',
                '@Parameter(title: "Music", requestValueDialog: "What would you like to play?")',
                'AppShortcut(intent: PlayIntent(), phrases: ["Play \\(.applicationName)"], shortTitle: "Play")',
                'return .result(dialog: "Playing your favourites.")',
            ]
        )

        self.assertEqual([literal.text for literal in words.prose_of(source)], ["Playing your favourites."])


if __name__ == "__main__":
    unittest.main()
