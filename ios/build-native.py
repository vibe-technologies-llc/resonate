import argparse
import os
from pathlib import Path
import shutil
import subprocess

TRIPLE = "aarch64-apple-ios"
DEPLOYMENT_TARGET = "26.0"
LIBRARY = "libresonate_mobile.a"
MODULE_MAP = """module ResonateNativeFFI {
    header "ResonateNativeFFI.h"
    link "resonate_mobile"
    export *
}
"""

parser = argparse.ArgumentParser()
parser.add_argument("--profile", choices=("debug", "release"), default="debug")
parser.add_argument("--sdk", type=Path, help="the iPhoneOS.sdk to compile C dependencies against")
args = parser.parse_args()
repository = Path(__file__).resolve().parent.parent
target = repository / "ios/.cargo-target"
generated = repository / "ios/Generated"
cargo = shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo")


def iphone_sdk():
    if args.sdk:
        return args.sdk
    bundles = Path.home() / ".swiftpm/swift-sdks"
    for bundle in sorted(bundles.glob("darwin*.artifactbundle")):
        found = bundle / "Developer/Platforms/iPhoneOS.platform/Developer/SDKs/iPhoneOS.sdk"
        if (found / "SDKSettings.json").exists():
            return found
    raise SystemExit("No iPhoneOS SDK found: run `xtool setup` with Xcode's .xip, or pass --sdk")


def run(command, env):
    subprocess.run(command, cwd=repository, env=env, check=True)


sdk = iphone_sdk()
environment = os.environ.copy()
environment.update(
    CARGO_TARGET_DIR=str(target),
    RUSTFLAGS="",
    CARGO_PROFILE_DEV_DEBUG="0",
    SDKROOT=str(sdk),
    IPHONEOS_DEPLOYMENT_TARGET=DEPLOYMENT_TARGET,
)
key = TRIPLE.replace("-", "_")
environment.update({
    f"CC_{key}": shutil.which("clang") or "clang",
    f"AR_{key}": shutil.which("llvm-ar") or "llvm-ar",
    f"CFLAGS_{key}": f"--target=arm64-apple-ios{DEPLOYMENT_TARGET} -isysroot {sdk}",
})
command = [cargo, "rustc", "--locked", "-p", "resonate-mobile", "--lib", "--target", TRIPLE, "--crate-type", "staticlib"]
if args.profile == "release":
    command.append("--release")
run(command, environment)
library = target / TRIPLE / args.profile / LIBRARY

host = os.environ.copy()
host.update(CARGO_TARGET_DIR=str(target), RUSTFLAGS="")
run([cargo, "build", "--locked", "-p", "resonate-bindgen"], host)
bindings = target / "bindings"
shutil.rmtree(bindings, ignore_errors=True)
bindings.mkdir(parents=True)
bindgen = [str(target / "debug/resonate-bindgen"), "generate", "--language", "swift", "--no-format", "--config", "crates/resonate-mobile/uniffi.toml", "--out-dir", str(bindings)]
try:
    run(bindgen + ["--library", str(library)], host)
except subprocess.CalledProcessError:
    run([cargo, "build", "--locked", "-p", "resonate-mobile", "--lib"], host)
    run(bindgen + ["--library", str(target / "debug/libresonate_mobile.so")], host)

shutil.rmtree(generated, ignore_errors=True)
for folder in ("ResonateNativeFFI", "ResonateNative", "lib"):
    (generated / folder).mkdir(parents=True)
shutil.copy2(bindings / "ResonateNativeFFI.h", generated / "ResonateNativeFFI")
(generated / "ResonateNativeFFI/module.modulemap").write_text(MODULE_MAP)
shutil.copy2(bindings / "ResonateNative.swift", generated / "ResonateNative")
shutil.copy2(library, generated / "lib")
