import os
from pathlib import Path
import shutil
import subprocess
import sys

ios = Path(__file__).resolve().parent
repository = ios.parent
cargo = shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo")

environment = os.environ.copy()
environment.update(CARGO_TARGET_DIR=str(ios / ".cargo-target"), RUSTFLAGS="")

if not (ios / "Generated/ResonateNative/ResonateNative.swift").exists():
    raise SystemExit("test-logic.py: ios/Generated is missing; run `python3 ios/build-native.py` first")

subprocess.run([cargo, "build", "--locked", "-p", "resonate-mobile", "--lib"], cwd=repository, env=environment, check=True)

environment["RESONATE_HOST_TESTS"] = "1"
raise SystemExit(subprocess.run(["swift", "test", *sys.argv[1:]], cwd=ios, env=environment).returncode)
