import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request

import altstore

parser = argparse.ArgumentParser(
    description=(
        "Build the release IPA from a tagged commit, put it and the AltStore source on the tag's draft GitHub release, "
        "then push the tag so release.yml attaches the rest and publishes"
    )
)
parser.add_argument("tag", help="the release tag, made beforehand on the commit checked out, not yet pushed")
parser.add_argument("--remote", default="origin", help="the remote the tag is pushed to")
parser.add_argument("--xtool", type=Path, help="the xtool to build with, else the one on PATH or install.py's kept AppImage")
args = parser.parse_args()
sys.stdout.reconfigure(line_buffering=True)

ios = Path(__file__).resolve().parent
repository = ios.parent
kept = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache") / "resonate" / f"xtool-{platform.machine()}.AppImage"
ICON = "ios/Resonate/Icon.png"


def step(what):
    print(f"\n==> {what}", flush=True)


def run(command, cwd=ios):
    try:
        subprocess.run([str(part) for part in command], cwd=cwd, check=True)
    except subprocess.CalledProcessError as failure:
        raise SystemExit(f"release.py: `{' '.join(str(part) for part in command)}` failed with exit status {failure.returncode}")


def read(command, cwd=repository):
    answered = subprocess.run([str(part) for part in command], cwd=cwd, capture_output=True, text=True)
    if answered.returncode != 0:
        raise SystemExit(f"release.py: `{' '.join(str(part) for part in command)}` failed: {answered.stderr.strip()}")
    return answered.stdout.strip()


def xtool():
    if args.xtool:
        return args.xtool
    found = shutil.which("xtool")
    if found:
        return Path(found)
    if kept.exists():
        return kept
    raise SystemExit(f"release.py: no xtool on PATH or at {kept}; run `python3 ios/install.py` once or pass --xtool")


def declared_version():
    for line in (repository / "Cargo.toml").read_text().splitlines():
        if line.startswith('version = "'):
            return line.split('"')[1]
    raise SystemExit("release.py: the workspace manifest declares no version")


def checked(tag):
    version, release, candidate = altstore.parsed_tag(tag)
    if declared_version() != version:
        raise SystemExit(f"release.py: {tag} is not the manifest's version {declared_version()}")
    shown = plistlib.loads((ios / "Resonate/Info.plist").read_bytes())["CFBundleShortVersionString"]
    if shown != release:
        raise SystemExit(f"release.py: Info.plist says {shown}, not {release}")
    if read(["git", "status", "--porcelain"]):
        raise SystemExit("release.py: the working tree has changes; the IPA is built from the tagged commit alone")
    tagged = subprocess.run(["git", "rev-parse", "--verify", "--quiet", f"{tag}^{{commit}}"], cwd=repository, capture_output=True, text=True)
    if tagged.returncode != 0:
        raise SystemExit(f"release.py: there is no tag {tag}; make it on the commit to release first")
    if tagged.stdout.strip() != read(["git", "rev-parse", "HEAD"]):
        raise SystemExit(f"release.py: {tag} is not the commit checked out; check it out first")
    if not read(["git", "branch", "--remotes", "--contains", tagged.stdout.strip(), "--list", f"{args.remote}/*"]):
        raise SystemExit(f"release.py: {tag}'s commit is on no branch of {args.remote}; push it first, since the draft release targets it")
    if read(["git", "ls-remote", "--tags", args.remote, f"refs/tags/{tag}"]):
        raise SystemExit(f"release.py: {tag} is already on {args.remote}, so release.yml may publish before the IPA is there")
    return version, candidate, tagged.stdout.strip()


def repository_url():
    return read(["gh", "repo", "view", "--json", "url", "--jq", ".url"])


def previous_source(website):
    url = f"{website}/releases/latest/download/{altstore.SOURCE_ASSET}"
    request = urllib.request.Request(url, headers={"User-Agent": f"resonate/{declared_version()}"})
    try:
        with urllib.request.urlopen(request, timeout=30) as answer:
            return json.load(answer)
    except urllib.error.HTTPError as refused:
        if refused.code == 404:
            return None
        raise SystemExit(f"release.py: the last published source could not be read: {refused}")


def drafted(tag, commit, candidate):
    if subprocess.run(["gh", "release", "view", tag], cwd=repository, capture_output=True).returncode == 0:
        state = read(["gh", "release", "view", tag, "--json", "isDraft", "--jq", ".isDraft"])
        if state != "true":
            raise SystemExit(f"release.py: the release {tag} is published; an immutable release takes no new asset")
        return
    command = ["gh", "release", "create", tag, "--draft", "--target", commit, "--generate-notes", "--title", tag]
    if candidate:
        command.append("--prerelease")
    run(command, cwd=repository)


tag = args.tag
version, candidate, commit = checked(tag)
website = repository_url()

step("Building the native library in release")
run([sys.executable, ios / "build-native.py", "--profile", "release"])
step("Building the app in release")
run([xtool(), "dev", "build", "-c", "release"])
step("Writing the App Intents metadata and packing the IPA")
run([sys.executable, ios / "app-intents.py", "-c", "release", "--ipa"])

with tempfile.TemporaryDirectory() as staging:
    ipa = Path(staging) / altstore.ipa_name(version)
    shutil.copyfile(ios / "xtool/Resonate.ipa", ipa)
    assets = [ipa]
    info = altstore.bundled_info(ipa)
    if not candidate:
        step("Writing the AltStore source")
        entitlements = plistlib.loads((ios / "Resonate/Resonate.entitlements").read_bytes())
        entry = altstore.version_entry(
            info,
            date=datetime.date.today().isoformat(),
            notes_url=f"{website}/releases/tag/{tag}",
            download_url=f"{website}/releases/download/{tag}/{ipa.name}",
            size=ipa.stat().st_size,
        )
        icon_url = f"{website.replace('https://github.com/', 'https://raw.githubusercontent.com/')}/{tag}/{ICON}"
        made = altstore.source(info, entitlements, entry, previous=previous_source(website), website=website, icon_url=icon_url)
        source = Path(staging) / altstore.SOURCE_ASSET
        source.write_text(json.dumps(made, indent=2) + "\n")
        assets.append(source)

    step(f"Putting {', '.join(asset.name for asset in assets)} on the draft release {tag}")
    drafted(tag, commit, candidate)
    run(["gh", "release", "upload", tag, *assets, "--clobber"], cwd=repository)

step(f"Pushing {tag}, which runs release.yml")
run(["git", "push", args.remote, f"refs/tags/{tag}"], cwd=repository)
print(f"\n{tag}'s IPA waits on the draft; release.yml attaches the other assets and publishes it")
