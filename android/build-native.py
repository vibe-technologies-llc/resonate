import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

parser = argparse.ArgumentParser()
parser.add_argument("--sdk", required=True, type=Path)
parser.add_argument("--profile", choices=("debug", "release"), default="debug")
parser.add_argument("--output", type=Path)
args = parser.parse_args()
repository = Path(__file__).resolve().parent.parent
output = args.output or repository / "android/app/build/generated"
target = repository / "android/.cargo-target"
ndk = args.sdk / "ndk/28.2.13676358"
host = {"linux": "linux-x86_64", "darwin": "darwin-x86_64", "win32": "windows-x86_64"}.get(sys.platform)
if host is None:
    raise SystemExit(f"Android NDK host is not supported: {sys.platform}")
toolchain = ndk / "toolchains/llvm/prebuilt" / host / "bin"
compiler_suffix = ".cmd" if sys.platform == "win32" else ""
executable_suffix = ".exe" if sys.platform == "win32" else ""
if not toolchain.is_dir():
    raise SystemExit(f"Install NDK 28.2.13676358 in Android Studio's SDK Manager: {ndk}")
environment = os.environ.copy()
environment.update(CARGO_TARGET_DIR=str(target), RUSTFLAGS="", CARGO_PROFILE_DEV_DEBUG="0")
cargo = shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo")

def run(command, env):
    subprocess.run(command, cwd=repository, env=env, check=True)

for triple, abi in [("aarch64-linux-android", "arm64-v8a"), ("x86_64-linux-android", "x86_64")]:
    native = environment.copy()
    key = triple.replace("-", "_")
    native.update({
        f"CARGO_TARGET_{key.upper()}_LINKER": str(toolchain / f"{triple}31-clang{compiler_suffix}"),
        f"CC_{key}": str(toolchain / f"{triple}31-clang{compiler_suffix}"),
        f"CXX_{key}": str(toolchain / f"{triple}31-clang++{compiler_suffix}"),
        f"AR_{key}": str(toolchain / f"llvm-ar{executable_suffix}"),
        "ANDROID_NDK_HOME": str(ndk),
        "ANDROID_NDK_ROOT": str(ndk),
        "RUSTFLAGS": "-C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,--no-undefined",
    })
    command = [cargo, "build", "--locked", "-p", "resonate-mobile", "--lib", "--target", triple]
    if args.profile == "release":
        native["CARGO_PROFILE_RELEASE_STRIP"] = "debuginfo"
        command.append("--release")
    run(command, native)
    destination = output / "jniLibs" / abi
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copy2(target / triple / args.profile / "libresonate_mobile.so", destination)
    shutil.copy2(toolchain.parent / "sysroot/usr/lib" / triple / "libc++_shared.so", destination)

run([cargo, "build", "--locked", "-p", "resonate-bindgen"], environment)
shutil.rmtree(output / "uniffi", ignore_errors=True)
(output / "uniffi").mkdir(parents=True, exist_ok=True)
run([str(target / f"debug/resonate-bindgen{executable_suffix}"), "generate", "--library", str(target / "x86_64-linux-android" / args.profile / "libresonate_mobile.so"), "--language", "kotlin", "--no-format", "--config", "crates/resonate-mobile/uniffi.toml", "--out-dir", str(output / "uniffi")], environment)
