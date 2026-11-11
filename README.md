# Resonate

A music player for people who care how their files reach the speaker. Linux is the main platform,
with Windows, macOS, Android and iOS builds sharing the same Rust engine.

On Linux it talks to PipeWire directly. It reads the rates a sink offers, switches the graph to the
file's own rate where the hardware accepts it, and resamples with its own filters where it doesn't.
A lossless file stays bit-accurate wherever the device allows. ReplayGain, a true-peak limiter,
noise-shaped dither and DoP for DSD all run in that one chain.

Also in the box:

- A GPUI window, native on Wayland, with albums, artists, playlists, search, synced lyrics, an
  equaliser and an analysis pane.
- A SQLite catalog filled from tags and cue sheets. MusicBrainz, the Cover Art Archive and LRCLIB
  fill in releases, covers and lyrics, and only strict matches are written.
- A per-track study (waveform, spectrum, BS.1770 loudness, DR, Chromaprint) that flags "lossless"
  files that were upsampled or transcoded from lossy.
- A vault that keeps the smallest bit-exact copy of each track, validated by decoding it back.
- An equaliser bound per output device, reading EqualizerAPO and AutoEq profiles.
- UPnP and Google Cast renderers as outputs beside the local ones.
- Sync between your own devices, shared playlists and listening parties, all without accounts.
- Song recognition, MPRIS, Discord presence and an MCP server.

Formats: FLAC, ALAC, AAC, MP3, Vorbis, Opus, WavPack, Monkey's Audio, WAV, AIFF, CAF, Matroska,
MP4 and DSD (DSF, DFF). Cue sheets are read from a sidecar or from a FLAC's own `CUESHEET`.

## Install

Each [GitHub release](https://github.com/vibe-technologies-llc/resonate/releases) carries:

| Platform | Asset |
|---|---|
| Fedora 44 | `.rpm` |
| Ubuntu 24.04 | `.deb` |
| Arch | `.pkg.tar.zst`, or `resonate-player-git` from the AUR |
| Windows (x64, ARM64) | installer `.exe` or `.zip` |
| macOS 13+ (Apple Silicon) | `.dmg` |
| Android 13+ | `.apk` |
| iOS 26+ | `.ipa`, plus an AltStore/SideStore source |

The Linux packages target x86-64-v3; older CPUs need a source build. The Windows and macOS builds
are unsigned, so SmartScreen and Gatekeeper will ask on first run. The IPA is signed ad hoc, so you
sign it with your own Apple ID when sideloading. The macOS build has been type-checked and tested in
CI but not yet run on a real Mac.

## Build

Linux needs PipeWire, Wayland, xkbcommon, Vulkan, fontconfig, freetype, clang and pkg-config. On
Arch:

```sh
sudo pacman -S --needed rust clang pkgconf pipewire libxkbcommon libxkbcommon-x11 wayland \
  fontconfig freetype2 vulkan-icd-loader vulkan-headers
cargo build --release -p resonate
```

The `resonate` binary has the features `ui`, `online`, `mcp`, `discord` and `sync`, all on by
default. Without `online` there is no HTTP client in the build at all. To build the audio stack
without the window:

```sh
cargo build --release --workspace --exclude resonate-ui --no-default-features
```

`.cargo/config.toml` sets `target-cpu=native`, so a binary built this way may not run on another
machine. The YouTube Music sign-in window is a separate binary (`-p resonate-signin`) because it
links WebKitGTK.

Packaging recipes live in `packaging/`: `PKGBUILD`, `resonate.spec`, `deb/build.sh`, the Flatpak
manifest `org.resonate.Resonate.yml`, `windows/build.ps1` and `macos/build.sh`.

Android: open `android/` in Android Studio (SDK 36, NDK 28.2.13676358, the
`aarch64-linux-android` and `x86_64-linux-android` Rust targets) and run it. Gradle builds the Rust
library and Kotlin bindings itself. From a terminal, `android/gradlew -p android :app:installDebug`.

iOS: built and installed from Linux with [xtool](https://github.com/xtool-org/xtool) and the SDK
from your Xcode download. `python3 ios/install.py` does the whole thing for an attached device.

## Use

```sh
resonate                      # the window, on the queue you left
resonate song.flac album/     # the window, with these queued
resonate play song.flac       # play in the terminal
resonate scan ~/Music         # add a folder to the library
resonate sinks                # outputs and the rates each takes
resonate explain song.flac    # how a file will be played on the chosen output
resonate analyse song.flac    # is this lossless file really lossless
```

There are about forty subcommands. `resonate --help` lists them, and the man pages and shell
completions are generated from the same definitions.

Settings live in `$XDG_CONFIG_HOME/resonate/config.toml` (`%APPDATA%\resonate` on Windows,
`~/Library/Application Support/resonate` on macOS). The settings pane edits the same file and keeps
your comments and key order. `man 5 resonate-config` documents every key.

## Privacy

Requests identify as `resonate/<version>` and nothing else. `online = false` stops every network
request. With it on, a scan looks tracks up on MusicBrainz and its neighbours (Cover Art Archive,
Wikidata, Wikimedia Commons, LRCLIB, Better Lyrics, Deezer, Apple Music, Spotify, SoundCloud).
Song recognition sends Shazam a signature, not the audio. Wanted tracks are asked of the hosted
hifi-api and Monochrome services unless `hosted-services` is off. ListenBrainz, Last.fm, AudD, Subsonic, Jellyfin, TIDAL and Discord are contacted only once you
fill in their settings, and all of them start empty. The sync server stores only records sealed
with keys it never sees.

## Licence

AGPL-3.0-or-later. Copyright (C) 2026 Vibe Technologies LLC. See [LICENSE](LICENSE).
