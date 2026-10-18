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
cd ios && xtool dev build && python3 app-intents.py --ipa
```

The first command writes `ios/Generated/` (bindings, module map, `libresonate_mobile.a`); rerun it
after any change under `crates/`. `--profile release` builds the optimised archive. `xtool dev
build` leaves `ios/xtool/Resonate.app` (entitlements applied ad hoc); `app-intents.py` writes the
App Intents metadata Siri and Shortcuts read into it, which Xcode would have written (`ios.md`'s
*App Intents metadata*), and `--ipa` packs `ios/xtool/Resonate.ipa` beside it. Give both the same
configuration: `xtool dev build -c release` goes with `app-intents.py -c release`. Never
`xtool dev build --ipa`: it packs the app before the metadata is in it.

## On a device

```bash
python3 ios/install.py
```

One command does the whole run: it fetches the latest xtool AppImage from xtool's GitHub release
(checked against the release's sha256 digest, kept in `~/.cache/resonate/`, downloaded again only
when the release moves), checks that xtool is signed in and the `darwin` SDK is installed, builds
the native library and the app in release (`-c debug` for debug), writes the App Intents metadata
and installs with `xtool install` (`--udid` picks one of several iPhones). By hand, the same is:

```bash
xtool devices
cd ios && xtool dev build && python3 app-intents.py && xtool install xtool/Resonate.app
```

`xtool install` signs with the listener's team and installs; open the app on the phone, or
`xtool launch org.resonate.ios`. Never `xtool dev run`: it builds the bundle again without the App
Intents metadata, so Siri and Shortcuts would not see the app's intents. A free Apple ID's profile
lasts seven days; the device must trust the developer in Settings › General › VPN & Device
Management the first time. Logs: `idevicesyslog | grep -i resonate` (libimobiledevice).

## Putting music on it

Drop files into Files › On My iPhone › Resonate, or choose a folder from the app (Songs or Library,
the folder button). Never point it at the listener's only copy of a library for write tests.
