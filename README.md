# Resonate

A music player for audio enthusiasts, targeting Linux first, with high-fidelity, high-performance
playback as the guiding constraint.

- A **native PipeWire client** rather than a PulseAudio or ALSA compatibility layer. It reads the
  rates a sink advertises, switches the graph to the source's own rate where the hardware takes it,
  and resamples with its own polyphase filters where it does not — so a lossless file reaches the
  device bit-accurate wherever the device allows it. ReplayGain, a true-peak guard, dither with
  noise shaping and DoP for DSD sit in the same chain.
- A **GPUI front end** running natively on Wayland: a library with albums, artists, playlists,
  favourites and suggestions, a search grammar, synced lyrics, an analysis pane, an equaliser and a
  settings pane that writes `config.toml` back.
- **MPRIS on the session bus**, which is how media keys, notifications and every desktop's own
  player controls reach the transport — the player itself reads no key. A second `resonate`
  reaches a running one over the same interface.
- **A catalog that knows what it holds.** A scan reads tags, cue sheets and embedded pictures into
  SQLite; an optional lookup against MusicBrainz, the Cover Art Archive and LRCLIB fills in
  releases, covers, artist portraits and lyrics, lists the tracks of a release you are missing, and
  writes only what matched strictly.
- **Every track studied once.** A whole decode yields a waveform, a spectrum, BS.1770 loudness, a
  DR reading and a Chromaprint, and a verdict on whether a "lossless" file is really an upsampled
  or transcoded lossy one.
- **A managed vault**: an archive of the smallest bit-exact copy of each track, stripped of tags
  and validated by decoding it back, that never writes to the library it was imported from.
- **An equaliser** bound per output device, reading EqualizerAPO and AutoEq GraphicEQ files and
  fetching the AutoEq measurement for a pair of headphones.
- **Listen**: record what the desktop or a microphone is playing and name the song through Shazam,
  AudD or AcoustID.
- Optional **Discord presence** and a **Model Context Protocol** server that hands the catalog and
  the running player to a language model.

Until the first release is published, build from source.

## Building

```
cargo build --release --workspace                                    # everything, the window included
cargo build --release --workspace --exclude resonate-ui --no-default-features   # the audio stack alone
```

The binary's features are `ui`, `online`, `mcp` and `discord`, all on by default. Turning `online`
off builds with no HTTP client in the tree.

The audio stack needs PipeWire and a C toolchain for its bindings; the window adds Wayland,
xkbcommon, Vulkan, fontconfig and freetype. On Arch that is
`pipewire libpipewire libxkbcommon wayland fontconfig freetype2 vulkan-icd-loader` to run, and
`rust cargo clang pkgconf vulkan-headers libxkbcommon-x11` to build — gpui links the X11 half of
xkbcommon even in a Wayland-only build, and the linker drops it from the binary again.
`packaging/PKGBUILD` is the package, published on the AUR as `resonate-player-git`.

On Fedora 44, build the RPM from a committed checkout with `packaging/resonate.spec`:

```
sudo dnf install git rpm-build cargo rust clang gcc pkgconf-pkg-config pipewire-devel \
  wayland-devel libxkbcommon-devel libxkbcommon-x11-devel fontconfig-devel \
  freetype-devel vulkan-headers vulkan-loader-devel
mkdir -p "$HOME/rpmbuild/SOURCES"
git archive --format=tar.gz --prefix=resonate-0.1.0/ HEAD \
  -o "$HOME/rpmbuild/SOURCES/resonate-0.1.0.tar.gz"
rpmbuild -ba packaging/resonate.spec
sudo dnf install "$HOME"/rpmbuild/RPMS/$(uname -m)/resonate-0.1.0-1.fc44.$(uname -m).rpm
```

The spec builds the default feature set and installs the launcher, icon, metainfo, man pages and
shell completions. The build needs access to crates.io for Cargo's locked dependencies.
Publishing a GitHub release tagged `v0.1.0` or `0.1.0` builds the Fedora 44 x86_64 RPM and attaches
it and the source RPM to that release. The tag must match the spec version. No release is
published yet.

`packaging/org.resonate.Resonate.yml` is the Flatpak. It builds this checkout with the
Freedesktop 25.08 SDK, and it needs `flatpak`, `flatpak-builder` and these runtimes:

```
flatpak install --user flathub org.freedesktop.Platform//25.08 org.freedesktop.Sdk//25.08 \
  org.freedesktop.Sdk.Extension.rust-stable//25.08
flatpak-builder --force-clean --user --install flatpak-build packaging/org.resonate.Resonate.yml
flatpak run org.resonate.Resonate
```

`.cargo/config.toml` builds for `target-cpu=native`, which is worth 1.4x to 1.7x on the resampler.
The Arch PKGBUILD keeps it for the machine building the package. The Fedora RPM and the Flatpak
select the architecture's baseline CPU so a build runs on other machines.

## Playing

```
resonate                          # the window, on the queue the last run left, paused where it
                                  #   stopped
resonate <files>                  # the window, with those files queued; paths or file:// URIs
resonate play <files>             # a queue on the command line, with transport keys on stdin
resonate queue <files>            # add them to the player already running, over MPRIS; --next,
                                  #   --play, --playlist <name> and --player <name>
resonate players                  # the players running on the session bus and what each plays
resonate sleep <spec>             # stop the running player after so many minutes, at the end of
                                  #   the track, or at the end of the queue
resonate share [file]             # what a track reads as when it is passed on, with one link
```

## The library

```
resonate scan <roots>             # walk directories into the library, then look them up
resonate roots                    # the roots a bare `scan` walks
resonate forget <roots>           # drop roots and every track scanned from them
resonate enrich                   # ask MusicBrainz about what has not been asked lately
resonate missing                  # the release tracks the library holds no file for
resonate wants                    # the tracks marked wanted, and where a delivery landed
resonate poll                     # ask the registered providers for what is wanted
resonate tag                      # the tags that would be written back to say what the lookup
                                  #   learned; --apply writes them
resonate organise                 # the moves that would file every track under a layout;
                                  #   --apply makes them
resonate vault                    # what the vault holds; --import, --verify, --prune, --release
resonate studies                  # what the study of every track found; --fakes, --suspects,
                                  #   --misnamed
resonate playlists                # the playlists the library holds
resonate playlist <name>          # play one, add to it, order it, export it, save a query as one
resonate import <sheets>          # read M3U, PLS and XSPF in
resonate stats                    # what was played and listened to most
resonate favourites               # the tracks, albums and artists marked a favourite
resonate suggest                  # the playlists the catalog suggests making out of what it holds
```

## Inspecting

```
resonate info <file>              # tags, ReplayGain, container layout, bitrate
resonate analyse <file>           # the verdict on a lossless claim, levels, loudness, DR, spectrum
resonate explain <file>           # the output plan for a file against the chosen sink
resonate sinks                    # what PipeWire advertises, and what the graph will switch to
resonate eq                       # the equaliser: bind a correction to a device, import one, or
                                  #   fetch the one AutoEq measured for your headphones
resonate listen                   # name the song the desktop or a microphone is playing
resonate mcp                      # serve the catalog and the player over the Model Context
                                  #   Protocol on stdin and stdout
```

`--help` is the whole of it, and the man pages and shell completions are generated from the same
grammar at build time.

## Formats

FLAC, ALAC, AAC (bare ADTS included), MP3, Vorbis, Opus, uncompressed PCM in WAV, AIFF and CAF,
Matroska and MP4 (fragmented included), and DSD in DSF and DFF — decoded to PCM or carried over DoP
where the device takes it. Cue sheets are read beside a single-file rip or out of a FLAC's own
`CUESHEET`, so one file is an album's worth of tracks. There is no WavPack or Monkey's Audio decoder
in the tree, so neither is offered anywhere.

## Settings

`$XDG_CONFIG_HOME/resonate/config.toml`, or `--config <FILE>`. A flag outranks the file and the file
outranks the defaults; an unknown key warns rather than failing the run. The settings pane writes
the same file back with `toml_edit`, so hand-written comments and key order survive.

The library lives in `$XDG_DATA_HOME/resonate/library.db`, or wherever `--library` names, and the
vault in `$XDG_DATA_HOME/resonate/vault`, created only when something is imported into it. The
library also keeps what was playing, so `resonate` with no files opens on the queue the last run
left — in the order it was playing, shuffle and all — paused where it stopped; `resume = false`
turns that off and discards what was kept.

## Privacy

A request identifies itself as `resonate/<version>` and nothing else. A contact is added only
where `contact` holds one.

`online` is on by default, and so is looking the library up after a scan. While online is on,
that lookup can reach MusicBrainz, the Cover Art Archive, Wikimedia Commons, Wikidata, LRCLIB,
AutoEq, Deezer and Apple Music. `online = false` stops every one of those.

Listen, while online is on, sends Shazam a signature of what was heard and asks for no key of
yours. AudD is sent the clip only where `audd-token` is set, and AcoustID is asked only where
`acoustid-key` is set.

Discord is told nothing unless `discord` is on and names an application of yours. What it is
told is handed to the Discord client on this machine, and the cover is a public Cover Art
Archive address.

## Licence

Copyright (C) 2026 Vibe Technologies LLC. Resonate is free software under the GNU Affero General
Public License, version 3 or later. You may use, study, share and change it, but anything you
distribute that is built from it, or offer to users over a network, must be released under the
same licence with its complete source. See [LICENSE](LICENSE).
