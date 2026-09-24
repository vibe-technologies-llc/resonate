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

It is early: there has been no release, so it is built from source.

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
`rust cargo clang pkgconf vulkan-headers` to build. `packaging/PKGBUILD` is the package, published on the AUR as
`resonate-player-git`.

`.cargo/config.toml` builds for `target-cpu=native`, which is worth 1.4x to 1.7x on the resampler and
is a *development* setting: anything producing a binary for another machine has to export
`RUSTFLAGS` over it, as the `PKGBUILD` does.

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

A request identifies itself as `resonate/<version>` and nothing else. `online = false` stops every
lookup; AcoustID and AudD are asked only once you set a key of your own, and Discord is told
nothing unless `discord` is on and names an application of yours.

## Licence

MIT. See [LICENSE](LICENSE).
