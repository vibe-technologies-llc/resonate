# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Security
- [M · blocked: design] A device that joins a shared playlist after a hostile editor compacted it
  takes the forged member keys from the snapshot: the first key heard for a `MemberId` stands, so
  devices that already held the list keep the true keys, but a newcomer's first is the forger's, and
  a removal it later makes seals the other members' seats to them; closing it needs `Member` ops
  signed by the member's own key, a change of the op format
- [S · blocked: design] A host on the network may take several addresses and so pass the media server's
  per-neighbour share of its answers (`door.rs`); nothing on a LAN names a host better than its address

## Catalog
- [S · 0.18.1] Let a queued artist's expansion be cancelled. **Low, builder.** Subsonic and Jellyfin
  now check a cancel between albums through `PlaysInPlace::songs_until`, but every caller still asks
  `songs_of` with none (`crates/resonate/src/served.rs`, `crates/resonate-ui/src/serving.rs`,
  `crates/resonate-mobile/src/served.rs`), so a listener who leaves waits for the cap. Pass the
  caller's cancel. Done when a test cancelling a queued artist stops before the next album.
- [S · 0.18.1] Pass over a ListenBrainz listen stamped below zero or with a fraction. **Low,
  builder.** A negative or non-integer `listened_at` fails the whole page
  (`crates/resonate-online/src/history.rs`), as a stamp past the clock did. Read the stamp leniently
  and pass the listen over. Done when a page holding one such listen imports the rest.
- [S · 0.18.1] Say a refused Wrapped year on the phones in their language. **Low, builder.**
  `resonate-mobile`'s `Error::Domain` is `transparent` over `resonate_core::Error`
  (`crates/resonate-mobile/src/error.rs`), so `YearNotHeld` and every other core refusal reach the
  phone as core's English diagnostic, against `language.md`'s rule that the mobile error is the
  phone's sentence. Map each core variant through the mobile catalog. Done when a refused year reads
  from the catalog in French.
- [M · blocked: platform] On macOS and Windows a retag that grows a hard-linked track's head while it
  plays still shifts the bytes under the decoder: neither can tell who holds a file
  (`Shifting::readers_can_be_asked` is false), so the copy is written back over the same inode as
  before rather than refusing every such write; Linux and Android wait for the readers under a lease

## Formats
- [M · blocked: symphonia] A file embedding a huge picture still costs one materialisation:
  symphonia 0.6.1 reads it into its own buffer before `probe_cover_art` can weigh it; its
  `limit_visual_bytes` is honoured by no reader

## macOS
- [S · blocked: a Mac] The desktop capture reads the aggregate's first input stream, never checked
  to be the tap: `Heard::from` takes `streams(Direction::Input).next()`
  (`crates/resonate-listen/src/coreaudio_capture.rs:62-81`) of the aggregate over the default output
  and the tap (`crates/resonate-apple/src/process_tap.rs:137-170`), so an output with inputs of its
  own, a USB headset, may record its microphone when the listener chose the desktop and send it to a
  recogniser; settling it needs a Mac with such an output

## Windows

## Command line
- [S · 0.18.1] Say `explain`'s settings through the catalog. **Low, builder.** `explain.rs` still
  prints the `Debug` of the filter phase, quality, tuning and shaping, as `info` and `readout` did
  before their words moved to the catalog. Give each its words. Done when `explain` prints no
  `Debug` spelling, in a test.
