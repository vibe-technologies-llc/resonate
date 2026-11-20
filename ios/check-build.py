import argparse
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess

import building

ios = Path(__file__).resolve().parent
kept = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache") / "resonate" / f"xtool-{platform.machine()}.AppImage"

parser = argparse.ArgumentParser(description="Build the app from nothing and fail on any warning in the output")
parser.add_argument("-c", "--configuration", choices=("debug", "release"), default="debug")
parser.add_argument("--xtool", type=Path, help="the xtool to build with, else the one on PATH or install.py's kept AppImage")
args = parser.parse_args()


def xtool():
    if args.xtool:
        return args.xtool
    found = shutil.which("xtool")
    if found:
        return Path(found)
    if kept.exists():
        return kept
    raise SystemExit("check-build.py: no xtool on PATH or at " + str(kept) + "; pass --xtool")


def check_widget(app):
    widget = app / "PlugIns/ResonateWidgets.appex"
    info = widget / "Info.plist"
    if not info.is_file():
        raise SystemExit("check-build.py: the app is missing ResonateWidgets.appex/Info.plist")
    app_info = plistlib.loads((app / "Info.plist").read_bytes())
    widget_info = plistlib.loads(info.read_bytes())
    for key, expected in building.widget_info(app_info).items():
        if widget_info.get(key) != expected:
            raise SystemExit(f"check-build.py: the widget's {key} differs from {expected!r}")
    if not widget_info["CFBundleIdentifier"].startswith(app_info["CFBundleIdentifier"] + "."):
        raise SystemExit("check-build.py: the widget's bundle identifier must extend the app's")
    for language in app_info["CFBundleLocalizations"]:
        for bundle in (app, widget):
            if not (bundle / f"{language}.lproj/Localizable.strings").is_file():
                raise SystemExit(f"check-build.py: {bundle.name} is missing its {language} strings")

    nm = "nm" if platform.system() == "Darwin" else "llvm-nm"
    executable = widget / widget_info["CFBundleExecutable"]
    listed = subprocess.check_output([nm, "-P", executable], text=True)
    symbols = dict(line.split()[:2] for line in listed.splitlines() if line.strip())
    if symbols.get("_main") != "T":
        raise SystemExit("check-build.py: the widget's Swift entry point was stripped by the linker")
    if symbols.get("_NSExtensionMain") != "U":
        raise SystemExit("check-build.py: the widget is missing Foundation's extension launcher")


shutil.rmtree(ios / ".build", ignore_errors=True)
built = subprocess.run([xtool(), "dev", "build", "-c", args.configuration], cwd=ios, capture_output=True, text=True)
output = built.stdout + built.stderr
warnings = building.warnings_in(output)
if built.returncode != 0 or not building.completed(output):
    raise SystemExit("\n".join(building.errors_in(output) or [output]) + f"\ncheck-build.py: the build failed with exit status {built.returncode}")
if warnings:
    raise SystemExit("\n".join(warnings) + f"\ncheck-build.py: the build gave {len(warnings)} warnings")
check_widget(ios / "xtool/Resonate.app")
print("The build gave no warnings and contains the linked WidgetKit extension")
