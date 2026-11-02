import plistlib
import re
import zipfile

SOURCE_ASSET = "resonate-altstore.json"
SOURCE_IDENTIFIER = "org.resonate.ios.source"
TINT = "6ea8fe"
CATEGORY = "entertainment"
NAME = "Resonate"
SUBTITLE = "A music player for audio enthusiasts"
DESCRIPTION = (
    "Resonate plays your own music library bit-accurately where the hardware allows, with lyrics, an equaliser, "
    "network renderers, Google Cast and devices kept in step."
)
PRIVACY_PREFIX = "NS"
PRIVACY_SUFFIX = "UsageDescription"
TAG = re.compile(r"^v(?P<version>(?P<release>\d+\.\d+\.\d+)(?:-(?P<candidate>(?:rc|beta|alpha)\d*))?)$")


class Refused(Exception):
    pass


def ipa_name(version):
    return f"resonate-{version}-ios.ipa"


def parsed_tag(tag):
    matched = TAG.match(tag)
    if matched is None:
        raise Refused(f"{tag} is not a release tag of the form vMAJOR.MINOR.PATCH[-rcN]")
    return matched["version"], matched["release"], matched["candidate"] is not None


def bundled_info(ipa):
    with zipfile.ZipFile(ipa) as archive:
        plists = [name for name in archive.namelist() if re.fullmatch(r"Payload/[^/]+\.app/Info\.plist", name)]
        if len(plists) != 1:
            raise Refused(f"{ipa} holds {len(plists)} app Info.plist files, not one")
        return plistlib.loads(archive.read(plists[0]))


def privacy_of(info):
    return {
        key: value
        for key, value in sorted(info.items())
        if key.startswith(PRIVACY_PREFIX) and key.endswith(PRIVACY_SUFFIX)
    }


def version_entry(info, *, date, notes_url, download_url, size):
    return {
        "version": info["CFBundleShortVersionString"],
        "buildVersion": info["CFBundleVersion"],
        "date": date,
        "localizedDescription": f"What changed: {notes_url}",
        "downloadURL": download_url,
        "size": size,
        "minOSVersion": info["MinimumOSVersion"],
    }


def same_build(one, other):
    return one["version"] == other["version"] and one["buildVersion"] == other["buildVersion"]


def earlier_versions(previous, bundle_identifier):
    if previous is None:
        return []
    apps = [app for app in previous.get("apps", []) if app.get("bundleIdentifier") == bundle_identifier]
    return apps[0].get("versions", []) if apps else []


def source(info, entitlements, entry, *, previous, website, icon_url):
    bundle_identifier = info["CFBundleIdentifier"]
    earlier = [version for version in earlier_versions(previous, bundle_identifier) if not same_build(version, entry)]
    return {
        "name": NAME,
        "identifier": SOURCE_IDENTIFIER,
        "subtitle": SUBTITLE,
        "description": DESCRIPTION,
        "iconURL": icon_url,
        "website": website,
        "tintColor": TINT,
        "apps": [
            {
                "name": NAME,
                "bundleIdentifier": bundle_identifier,
                "developerName": NAME,
                "subtitle": SUBTITLE,
                "localizedDescription": DESCRIPTION,
                "iconURL": icon_url,
                "tintColor": TINT,
                "category": CATEGORY,
                "screenshots": [],
                "versions": [entry, *earlier],
                "appPermissions": {
                    "entitlements": sorted(entitlements),
                    "privacy": privacy_of(info),
                },
            }
        ],
        "news": [],
    }
