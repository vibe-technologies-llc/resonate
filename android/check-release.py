import argparse
from pathlib import Path
import subprocess
import sys
import tempfile
import zipfile

parser = argparse.ArgumentParser()
parser.add_argument("--sdk", type=Path, required=True)
args = parser.parse_args()
repository = Path(__file__).resolve().parent.parent
host = {"linux": "linux-x86_64", "darwin": "darwin-x86_64", "win32": "windows-x86_64"}[sys.platform]
executable = ".exe" if sys.platform == "win32" else ""
readelf = args.sdk / "ndk/28.2.13676358/toolchains/llvm/prebuilt" / host / f"bin/llvm-readelf{executable}"
build_tools = args.sdk / "build-tools/36.0.0"
apk = repository / "android/app/build/outputs/apk/release/app-release-unsigned.apk"
kept_classes = {
    "Lcom/sun/jna/Pointer;": "peer",
    "Lorg/resonate/android/nativecore/MobilePlayer;": None,
}

subprocess.run([str(build_tools / f"zipalign{executable}"), "-c", "-P", "16", "4", str(apk)], check=True)
with tempfile.TemporaryDirectory(prefix="resonate-release-") as temporary:
    root = Path(temporary)
    with zipfile.ZipFile(apk) as archive:
        entries = archive.namelist()
        libraries = [entry for entry in entries if entry.startswith("lib/") and entry.endswith(".so")]
        other_abis = [entry for entry in libraries if not entry.startswith("lib/arm64-v8a/")]
        if not libraries or other_abis:
            raise SystemExit(f"the release APK packages {other_abis or 'no native libraries'}, not ARM64 alone")
        for entry in libraries:
            library = root / entry
            library.parent.mkdir(parents=True, exist_ok=True)
            library.write_bytes(archive.read(entry))
            headers = subprocess.check_output([str(readelf), "-lW", str(library)], text=True)
            loads = [line.split() for line in headers.splitlines() if line.strip().startswith("LOAD ")]
            if not loads or any(int(parts[-1], 16) < 16384 for parts in loads):
                raise SystemExit(f"{entry} is not aligned for 16 KB pages")
            print(f"{entry}: 16 KB page alignment passed")
        dumps = []
        for entry in entries:
            if entry.startswith("classes") and entry.endswith(".dex"):
                dex = root / entry
                dex.write_bytes(archive.read(entry))
                dumps.append(subprocess.check_output([str(build_tools / f"dexdump{executable}"), str(dex)], text=True))

classes = {}
for dump in dumps:
    for block in dump.split("\nClass #")[1:]:
        descriptor = next((line.split(":", 1)[1].strip().strip("'") for line in block.splitlines() if "Class descriptor" in line), None)
        if descriptor in kept_classes:
            classes[descriptor] = block
for descriptor, field in kept_classes.items():
    block = classes.get(descriptor)
    if block is None:
        raise SystemExit(f"R8 removed or renamed {descriptor}")
    if field is not None and f"name          : '{field}'" not in block:
        raise SystemExit(f"R8 renamed or removed {descriptor}.{field}, which JNA reads by name")
    print(f"{descriptor}{'.' + field if field else ''}: kept by R8")
