---
name: run-ios
description: Build, package, install and launch the Resonate iOS app from this Linux machine with xtool. Use whenever the iOS app needs to be built, put on an iPhone, or checked for real - the Swift package cannot build until the Rust library and bindings are generated.
---

# Running the iOS app

## Once per machine

- Swift 6.4 on `PATH` (`swift --version`) and the xtool AppImage, 1.22.2 or later
  (`xtool --version`; an older one on `PATH` may lack Xcode 27 support, so call the newer AppImage
  by its path). `zip` installed, which `--ipa` packs with.
- `xtool setup`: signs in (Apple ID or App Store Connect key) and builds the `darwin` Swift SDK from
  Xcode's `.xip`. `swift sdk list` must show `darwin`. Both steps are interactive: ask the listener.
- `rustup target add aarch64-apple-ios`.
- `usbmuxd` running for a device (`systemctl status usbmuxd`; udev starts it on plug-in).

## Build

```bash
python3 ios/build-native.py --profile debug
cd ios && xtool dev build --ipa
```

The first command writes `ios/Generated/` (bindings, module map, `libresonate_mobile.a`); rerun it
after any change under `crates/`. `--profile release` builds the optimised archive. The second
leaves `ios/xtool/Resonate.ipa` in place of the `.app` (unsigned unless `--sign`; entitlements are
applied when signing).

## On a device

```bash
xtool devices
cd ios && xtool dev run
```

`xtool dev run` signs with the listener's team, installs and launches. A free Apple ID's profile
lasts seven days; the device must trust the developer in Settings › General › VPN & Device
Management the first time. Logs: `idevicesyslog | grep -i resonate` (libimobiledevice).

## Putting music on it

Drop files into Files › On My iPhone › Resonate, or choose a folder from the app (Songs or Library,
the folder button). Never point it at the listener's only copy of a library for write tests.
