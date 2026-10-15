import argparse
import os
from pathlib import Path
import re
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

ACCENTS = ("mauve", "blue", "teal", "green", "amber", "peach", "red")
PACKAGED_STOPS = ('stop-color="#6ea8fe"', 'stop-color="#63d29b"')
ICON_SIDES = (("", 1024), ("@2x", 120), ("@3x", 180))
SWIFT_ACCENTS = """public struct ResonateAccent: Sendable, Hashable, Identifiable {{
    public let name: String
    public let hex: UInt32

    public var id: String {{ name }}
    public var iconName: String {{ "Icon" + name.capitalized }}
    public var alternateIcon: String {{ name.capitalized }}

    public static let every: [ResonateAccent] = [
{rows}
    ]
}}
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


def resonate_accents():
    theme = (repository / "crates/resonate-ui/src/theme.rs").read_text()
    flavour = theme[theme.index("static RESONATE"):]
    flavour = flavour[:flavour.index("});")]
    return {name: int(re.search(rf"{name}: 0x([0-9a-f]{{6}})", flavour).group(1), 16) for name in ACCENTS}


def lifted_toward_white():
    launcher = (repository / "crates/resonate-ui/src/launcher.rs").read_text()
    return float(re.search(r"LIFTED_TOWARD_WHITE: f32 = ([0-9.]+);", launcher).group(1))


def lifted(colour, toward_white):
    channels = [(colour >> shift) & 0xFF for shift in (16, 8, 0)]
    raised = [round((255 - value) * toward_white + value) for value in channels]
    return (raised[0] << 16) | (raised[1] << 8) | raised[2]


def draw_icons(into, accents):
    packaged = (repository / "packaging/resonate.svg").read_text()
    toward_white = lifted_toward_white()
    into.mkdir(parents=True)
    for name, colour in accents.items():
        drawing = packaged.replace(PACKAGED_STOPS[0], f'stop-color="#{lifted(colour, toward_white):06x}"', 1)
        drawing = drawing.replace(PACKAGED_STOPS[1], f'stop-color="#{colour:06x}"', 1)
        source = into / f"{name}.svg"
        source.write_text(drawing)
        for suffix, side in ICON_SIDES:
            run(["rsvg-convert", "--width", str(side), "--height", str(side), "--background-color", "#16181d",
                 "--output", str(into / f"Icon{name.capitalize()}{suffix}.png"), str(source)], os.environ.copy())
        source.unlink()


def swift_accents(accents):
    rows = ",\n".join(f'        ResonateAccent(name: "{name}", hex: 0x{colour:06x})' for name, colour in accents.items())
    return SWIFT_ACCENTS.format(rows=rows)


def run(command, env):
    subprocess.run(command, cwd=repository, env=env, check=True)


sdk = iphone_sdk()
environment = os.environ.copy()
environment.update(
    CARGO_TARGET_DIR=str(target),
    RUSTFLAGS="",
    CARGO_PROFILE_DEV_DEBUG="0",
    CARGO_PROFILE_RELEASE_PANIC="unwind",
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
accents = resonate_accents()
(generated / "ResonateNative/Accents.swift").write_text(swift_accents(accents))
draw_icons(generated / "icons", accents)
