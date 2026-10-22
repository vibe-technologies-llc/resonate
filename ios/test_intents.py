import copy
import json
from pathlib import Path
import tempfile
import unittest

import intents

FIXTURES = Path(__file__).resolve().parent / "fixtures"
AUDIO_STARTING = "com.apple.link.systemProtocol.AudioStarting"


def entries():
    return json.loads((FIXTURES / "Siri.swiftconstvalues.json").read_text())


def named(all_entries, type_name):
    return next(entry for entry in all_entries if entry["typeName"] == type_name)


def walked(node):
    yield node
    children = node.values() if isinstance(node, dict) else node if isinstance(node, list) else ()
    for child in children:
        yield from walked(child)


class Fixture(unittest.TestCase):
    def test_the_metadata_written_for_siri_swift_is_the_one_kept_beside_it(self):
        kept = json.loads((FIXTURES / "Siri.extract.json").read_text())

        self.assertEqual(json.loads(json.dumps(intents.metadata(entries()))), kept)

    def test_every_intent_is_an_audio_starting_action_that_says_a_dialog(self):
        written = intents.metadata(entries())

        self.assertEqual(
            sorted(written["actions"]),
            ["PlayFavouritesIntent", "PlayFromLibraryIntent", "ResumePlaybackIntent", "ShuffleLibraryIntent"],
        )
        for action in written["actions"].values():
            self.assertEqual(action["systemProtocols"], [AUDIO_STARTING], action["identifier"])
            self.assertEqual(action["systemProtocolMetadata"], [AUDIO_STARTING, {"empty": {}}], action["identifier"])
            self.assertEqual(action["outputFlags"], 4, action["identifier"])
            self.assertFalse(action["openAppWhenRun"], action["identifier"])
            self.assertEqual(action["supportedModes"], 1, action["identifier"])

    def test_a_string_parameter_is_written_with_the_types_a_string_resolves_from(self):
        written = intents.metadata(entries())

        parameter = written["actions"]["PlayFromLibraryIntent"]["parameters"][0]

        self.assertEqual(parameter["name"], "music")
        self.assertEqual(parameter["valueType"], intents.primitive(intents.STRING))
        self.assertEqual(
            [resolvable["valueType"] for resolvable in parameter["resolvableInputTypes"]][1:],
            [intents.primitive(intents.STRING), intents.primitive(intents.INTEGER)],
        )

    def test_every_shortcut_names_an_intent_and_every_phrase_says_the_app(self):
        written = intents.metadata(entries())

        self.assertEqual(len(written["autoShortcuts"]), 4)
        for shortcut in written["autoShortcuts"]:
            self.assertIn(shortcut["actionIdentifier"], written["actions"])
            self.assertTrue(shortcut["phraseTemplates"])
            for phrase in shortcut["phraseTemplates"]:
                self.assertIn("${applicationName}", phrase["key"])
        self.assertEqual(written["autoShortcutProviderMangledName"], named(entries(), "Resonate.ResonateShortcuts")["mangledTypeName"])

    def test_the_files_written_are_the_actions_data_and_the_tools_version(self):
        written = intents.metadata(entries())

        with tempfile.TemporaryDirectory() as folder:
            into = Path(folder) / "Metadata.appintents"
            intents.write(into, written)

            self.assertEqual(json.loads((into / "extract.actionsdata").read_text()), json.loads(json.dumps(written)))
            self.assertEqual(json.loads((into / "version.json").read_text()), {"toolsVersion": intents.TOOLS_VERSION, "version": "3.0"})


class Refusals(unittest.TestCase):
    def refused(self, changed):
        with self.assertRaises(intents.Unsupported):
            intents.metadata(changed)

    def test_an_entity_is_refused_because_ios_would_not_match_it_to_the_binary(self):
        changed = entries()
        named(changed, "Resonate.ResumePlaybackIntent")["conformances"].append("AppIntents.AppEntity")

        self.refused(changed)

    def test_a_protocol_the_generator_has_not_been_compared_against_is_refused(self):
        changed = entries()
        named(changed, "Resonate.ResumePlaybackIntent")["conformances"].append("AppIntents.OpenIntent")

        self.refused(changed)

    def test_a_parameter_that_is_not_a_string_is_refused(self):
        changed = entries()
        for node in walked(named(changed, "Resonate.PlayFromLibraryIntent")):
            if isinstance(node, dict) and str(node.get("type", "")).startswith("AppIntents.IntentParameter<"):
                node["type"] = "AppIntents.IntentParameter<Swift.Int>"

        self.refused(changed)

    def test_a_perform_returning_something_unknown_is_refused(self):
        changed = entries()
        for alias in named(changed, "Resonate.ResumePlaybackIntent")["associatedTypeAliases"]:
            if alias["typeAliasName"] == "PerformResult":
                alias["substitutedTypeName"] = "some AppIntents.OpensIntent"

        self.refused(changed)

    def test_a_phrase_without_the_apps_name_is_refused(self):
        changed = entries()
        provider = named(changed, "Resonate.ResonateShortcuts")
        phrase = next(node for node in walked(provider) if isinstance(node, dict) and node.get("valueKind") == "InterpolatedStringLiteral")
        phrase.clear()
        phrase.update({"valueKind": "RawLiteral", "value": "Play music"})

        self.refused(changed)

    def test_a_title_that_is_not_a_literal_is_refused(self):
        changed = copy.deepcopy(entries())
        intent = named(changed, "Resonate.ResumePlaybackIntent")
        title = next(prop for prop in intent["properties"] if prop["label"] == "title")
        title["valueKind"] = "Runtime"

        self.refused(changed)


if __name__ == "__main__":
    unittest.main()
