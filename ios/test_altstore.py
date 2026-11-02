from pathlib import Path
import io
import plistlib
import tempfile
import unittest
import zipfile

import altstore

INFO = {
    "CFBundleIdentifier": "org.resonate.ios",
    "CFBundleShortVersionString": "0.5.0",
    "CFBundleVersion": "6",
    "MinimumOSVersion": "26.0",
    "NSMicrophoneUsageDescription": "Listens",
    "NSLocalNetworkUsageDescription": "Finds",
    "NSBonjourServices": ["_resonate-sync._udp"],
}
WEBSITE = "https://example.org/resonate"
ICON = "https://example.org/icon.png"


def entry(version, build):
    return altstore.version_entry(
        {**INFO, "CFBundleShortVersionString": version, "CFBundleVersion": build},
        date="2026-11-02",
        notes_url=f"{WEBSITE}/releases/tag/v{version}",
        download_url=f"{WEBSITE}/releases/download/v{version}/{altstore.ipa_name(version)}",
        size=1234,
    )


def written(ipa, plists):
    with zipfile.ZipFile(ipa, "w") as archive:
        for name, info in plists.items():
            archive.writestr(name, plistlib.dumps(info))


class Tags(unittest.TestCase):
    def test_a_release_and_a_candidate_are_read(self):
        self.assertEqual(altstore.parsed_tag("v0.5.0"), ("0.5.0", "0.5.0", False))
        self.assertEqual(altstore.parsed_tag("v0.5.0-rc2"), ("0.5.0-rc2", "0.5.0", True))

    def test_anything_else_is_refused(self):
        for tag in ("0.5.0", "v0.5", "v0.5.0-final", "v0.5.0.1"):
            with self.assertRaises(altstore.Refused):
                altstore.parsed_tag(tag)


class Bundle(unittest.TestCase):
    def test_the_apps_info_is_read_from_the_payload(self):
        with tempfile.TemporaryDirectory() as folder:
            ipa = Path(folder) / "app.ipa"
            written(ipa, {"Payload/Resonate.app/Info.plist": INFO})

            self.assertEqual(altstore.bundled_info(ipa)["CFBundleVersion"], "6")

    def test_a_payload_without_exactly_one_app_is_refused(self):
        with tempfile.TemporaryDirectory() as folder:
            empty = Path(folder) / "empty.ipa"
            nested = Path(folder) / "nested.ipa"
            written(empty, {})
            written(nested, {"Payload/Resonate.app/Frameworks/Inner.framework/Info.plist": INFO})

            for ipa in (empty, nested):
                with self.assertRaises(altstore.Refused):
                    altstore.bundled_info(ipa)


class Source(unittest.TestCase):
    def test_the_new_version_leads_and_every_earlier_one_follows(self):
        previous = altstore.source(INFO, [], entry("0.4.0", "5"), previous=None, website=WEBSITE, icon_url=ICON)

        made = altstore.source(INFO, [], entry("0.5.0", "6"), previous=previous, website=WEBSITE, icon_url=ICON)

        versions = made["apps"][0]["versions"]
        self.assertEqual([version["version"] for version in versions], ["0.5.0", "0.4.0"])

    def test_the_same_build_published_again_is_listed_once(self):
        previous = altstore.source(INFO, [], entry("0.5.0", "6"), previous=None, website=WEBSITE, icon_url=ICON)

        made = altstore.source(INFO, [], entry("0.5.0", "6"), previous=previous, website=WEBSITE, icon_url=ICON)

        self.assertEqual(len(made["apps"][0]["versions"]), 1)

    def test_another_apps_versions_are_not_carried(self):
        stranger = {"apps": [{"bundleIdentifier": "org.example.other", "versions": [entry("9.9.9", "1")]}]}

        made = altstore.source(INFO, [], entry("0.5.0", "6"), previous=stranger, website=WEBSITE, icon_url=ICON)

        self.assertEqual([version["version"] for version in made["apps"][0]["versions"]], ["0.5.0"])

    def test_the_permissions_are_the_entitlements_and_every_usage_description(self):
        made = altstore.source(INFO, ["b", "a"], entry("0.5.0", "6"), previous=None, website=WEBSITE, icon_url=ICON)

        permissions = made["apps"][0]["appPermissions"]
        self.assertEqual(permissions["entitlements"], ["a", "b"])
        self.assertEqual(
            permissions["privacy"],
            {"NSLocalNetworkUsageDescription": "Finds", "NSMicrophoneUsageDescription": "Listens"},
        )

    def test_a_version_names_its_download_size_and_minimum(self):
        made = entry("0.5.0", "6")

        self.assertEqual(made["downloadURL"], f"{WEBSITE}/releases/download/v0.5.0/resonate-0.5.0-ios.ipa")
        self.assertEqual(made["size"], 1234)
        self.assertEqual(made["minOSVersion"], "26.0")


class Entitlements(unittest.TestCase):
    def test_the_apps_entitlements_file_reads_as_a_dictionary(self):
        path = Path(__file__).resolve().parent / "Resonate/Resonate.entitlements"

        read = plistlib.load(io.BytesIO(path.read_bytes()))

        self.assertIsInstance(read, dict)
        self.assertIn("com.apple.security.hardened-process", read)


if __name__ == "__main__":
    unittest.main()
