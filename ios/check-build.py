import argparse
import os
from pathlib import Path
import platform
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


shutil.rmtree(ios / ".build", ignore_errors=True)
built = subprocess.run([xtool(), "dev", "build", "-c", args.configuration], cwd=ios, capture_output=True, text=True)
output = built.stdout + built.stderr
warnings = building.warnings_in(output)
if built.returncode != 0 or not building.completed(output):
    raise SystemExit("\n".join(building.errors_in(output) or [output]) + f"\ncheck-build.py: the build failed with exit status {built.returncode}")
if warnings:
    raise SystemExit("\n".join(warnings) + f"\ncheck-build.py: the build gave {len(warnings)} warnings")
print("The build gave no warnings")
