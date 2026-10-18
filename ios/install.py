import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import subprocess
import sys
import urllib.request

LATEST_RELEASE = "https://api.github.com/repos/xtool-org/xtool/releases/latest"
CHUNK = 1 << 20

parser = argparse.ArgumentParser(
    description="Fetch the latest xtool AppImage, build Resonate for iOS and install it on the attached iPhone"
)
parser.add_argument("-c", "--configuration", choices=("debug", "release"), default="release")
parser.add_argument("-u", "--udid", help="the iPhone to install on, where more than one is attached")
parser.add_argument(
    "--xtool-dir",
    type=Path,
    default=Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache") / "resonate",
    help="where the xtool AppImage is kept between runs",
)
args = parser.parse_args()
sys.stdout.reconfigure(line_buffering=True)
ios = Path(__file__).resolve().parent
version = plistlib.loads((ios / "Resonate/Info.plist").read_bytes())["CFBundleShortVersionString"]
agent = f"resonate/{version}"


def step(what):
    print(f"\n==> {what}", flush=True)


def run(command, cwd=ios):
    try:
        subprocess.run([str(part) for part in command], cwd=cwd, check=True)
    except subprocess.CalledProcessError as failure:
        raise SystemExit(f"install.py: `{' '.join(str(part) for part in command)}` failed with exit status {failure.returncode}")


def asked(url):
    return urllib.request.Request(url, headers={"User-Agent": agent, "Accept": "application/vnd.github+json"})


def sha256_of(path):
    digest = hashlib.sha256()
    with path.open("rb") as read:
        while chunk := read.read(CHUNK):
            digest.update(chunk)
    return digest.hexdigest()


def latest_xtool():
    name = f"xtool-{platform.machine()}.AppImage"
    with urllib.request.urlopen(asked(LATEST_RELEASE), timeout=30) as answer:
        release = json.load(answer)
    asset = next((asset for asset in release["assets"] if asset["name"] == name), None)
    if asset is None:
        raise SystemExit(f"install.py: xtool {release['tag_name']} publishes no {name}")
    algorithm, _, expected = (asset.get("digest") or "").partition(":")
    if algorithm != "sha256" or not expected:
        raise SystemExit(f"install.py: xtool {release['tag_name']}'s {name} carries no sha256 digest to check it against")

    kept = args.xtool_dir / name
    if kept.exists() and sha256_of(kept) == expected:
        print(f"xtool {release['tag_name']} is already at {kept}")
        return kept

    print(f"Downloading xtool {release['tag_name']} ({asset['size'] / CHUNK:.0f} MiB) to {kept}")
    args.xtool_dir.mkdir(parents=True, exist_ok=True)
    partial = kept.with_name(kept.name + f".part-{os.getpid()}")
    digest = hashlib.sha256()
    try:
        with urllib.request.urlopen(asked(asset["browser_download_url"]), timeout=60) as answer, partial.open("wb") as written:
            while chunk := answer.read(CHUNK):
                digest.update(chunk)
                written.write(chunk)
        if digest.hexdigest() != expected:
            raise SystemExit(f"install.py: the downloaded {name} does not match its published sha256 digest; nothing was kept")
        partial.chmod(0o755)
        partial.replace(kept)
    finally:
        partial.unlink(missing_ok=True)
    return kept


def ready(xtool):
    status = subprocess.run([xtool, "auth", "status"], capture_output=True, text=True)
    if not status.stdout.startswith("Logged in"):
        raise SystemExit(f"install.py: xtool is not signed in to an Apple ID; run `{xtool} setup` once, then this again")
    sdks = subprocess.run(["swift", "sdk", "list"], capture_output=True, text=True)
    if "darwin" not in sdks.stdout.split():
        raise SystemExit(f"install.py: the darwin Swift SDK is missing; run `{xtool} setup` with Xcode's .xip, then this again")


step("Fetching the latest xtool")
xtool = latest_xtool()
run([xtool, "--version"])
ready(xtool)

step(f"Building the native library ({args.configuration})")
run([sys.executable, ios / "build-native.py", "--profile", args.configuration])

step(f"Building the app ({args.configuration})")
run([xtool, "dev", "build", "-c", args.configuration])

step("Writing the App Intents metadata")
run([sys.executable, ios / "app-intents.py", "-c", args.configuration])

step("Installing on the iPhone")
run([xtool, "install", *(["--udid", args.udid] if args.udid else []), ios / "xtool/Resonate.app"])
