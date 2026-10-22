import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

from intents import MODULE, metadata, write

parser = argparse.ArgumentParser(
    description="Write Metadata.appintents into the built app from the compiler's const values, in place of Xcode's appintentsmetadataprocessor"
)
parser.add_argument("-c", "--configuration", choices=("debug", "release"), default="debug")
parser.add_argument("--ipa", action="store_true", help="also pack the app as xtool/Resonate.ipa")
args = parser.parse_args()
ios = Path(__file__).resolve().parent
app = ios / "xtool/Resonate.app"


def const_values():
    configuration = args.configuration.capitalize()
    sources = {path.stem for path in (ios / "Sources" / MODULE).glob("*.swift")}
    written = list((ios / ".build").glob(f"**/{MODULE}.build/{configuration}-iphoneos/{MODULE}-t.build/**/*.swiftconstvalues"))
    whole_module = [path for path in written if path.stem == f"{MODULE}-primary"]
    found = whole_module or [path for path in written if path.stem in sources]
    if not found:
        raise SystemExit(f"app-intents.py: no const values for a {args.configuration} build under {ios / '.build'}; run `xtool dev build -c {args.configuration}` first")
    return [entry for path in sorted(found) for entry in json.loads(path.read_text())]


def pack(bundle):
    ipa = bundle.with_suffix(".ipa")
    with tempfile.TemporaryDirectory() as staging:
        payload = Path(staging) / "Payload"
        payload.mkdir()
        shutil.copytree(bundle, payload / bundle.name, symlinks=True)
        ipa.unlink(missing_ok=True)
        subprocess.run(["zip", "-qry", str(ipa), "Payload"], cwd=staging, check=True)
    return ipa


if not (app / "Info.plist").exists():
    raise SystemExit(f"app-intents.py: no app at {app}; run `xtool dev build -c {args.configuration}` (without --ipa) first")
written = metadata(const_values())
write(app / "Metadata.appintents", written)
print(f"Wrote {len(written['actions'])} intents and {len(written['autoShortcuts'])} App Shortcuts into {app / 'Metadata.appintents'}")
if args.ipa:
    print(f"Packed {pack(app)}")
