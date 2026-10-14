import argparse
import os
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
apk = repository / "android/app/build/outputs/apk/debug/app-debug.apk"
with tempfile.TemporaryDirectory(prefix="resonate-native-") as temporary:
    with zipfile.ZipFile(apk) as archive:
        for entry in archive.namelist():
            if entry.startswith("lib/") and entry.endswith(".so"):
                library = Path(temporary) / entry
                library.parent.mkdir(parents=True, exist_ok=True)
                library.write_bytes(archive.read(entry))
                headers = subprocess.check_output([str(readelf), "-lW", str(library)], text=True)
                loads = [line.split() for line in headers.splitlines() if line.strip().startswith("LOAD ")]
                if not loads or any(int(parts[-1], 16) < 16384 for parts in loads):
                    raise SystemExit(f"{entry} is not aligned for 16 KB pages")
                print(f"{entry}: 16 KB page alignment passed")
subprocess.run([str(args.sdk / f"build-tools/36.0.0/zipalign{executable}"), "-c", "-P", "16", "4", str(apk)], check=True)
for abi in ("arm64-v8a", "x86_64"):
    folder = repository / "android/app/build/generated/jniLibs" / abi
    for name in ("libresonate_mobile.so", "libc++_shared.so"):
        library = folder / name
        headers = subprocess.check_output([str(readelf), "-lW", str(library)], text=True)
        loads = [line.split() for line in headers.splitlines() if line.strip().startswith("LOAD ")]
        if not loads or any(int(parts[-1], 16) < 16384 for parts in loads):
            raise SystemExit(f"{abi}/{name} is not aligned for 16 KB pages")
        print(f"{abi}/{name}: 16 KB page alignment passed")
    environment = os.environ.copy()
    triple = "aarch64-linux-android" if abi == "arm64-v8a" else "x86_64-linux-android"
    toolchain = readelf.parent
    compiler = ".cmd" if sys.platform == "win32" else ""
    key = triple.replace("-", "_")
    environment.update({
        "CARGO_TARGET_DIR": str(repository / "android/.cargo-target"),
        f"CARGO_TARGET_{key.upper()}_LINKER": str(toolchain / f"{triple}31-clang{compiler}"),
        f"CC_{key}": str(toolchain / f"{triple}31-clang{compiler}"),
        f"CXX_{key}": str(toolchain / f"{triple}31-clang++{compiler}"),
        f"AR_{key}": str(toolchain / f"llvm-ar{executable}"),
        "RUSTFLAGS": "-C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,--no-undefined",
        "CARGO_PROFILE_DEV_DEBUG": "0",
    })
    subprocess.run(["cargo", "clippy", "--locked", "-p", "resonate-mobile", "--lib", "--target", triple, "--", "-D", "warnings"], cwd=repository, env=environment, check=True)
