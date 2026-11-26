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
- [M · blocked: platform] On macOS and Windows a retag that grows a hard-linked track's head while it
  plays still shifts the bytes under the decoder: neither can tell who holds a file
  (`Shifting::readers_can_be_asked` is false), so the copy is written back over the same inode as
  before rather than refusing every such write; Linux and Android wait for the readers under a lease

## Equaliser
- [M · 0.18.1] A profile kept or edited past the import's lift plays at any gain. **Medium,
  architect.** `Store::read_in` refuses a lift over `BOOST_AT_MOST_DB` (24 dB, `PeakTooHigh`), as
  the phones' parser does, but `Store::read`, `Store::own`, `read_profile`, the AutoEq reads and the
  desktop editor do not, so 32 bands of +40 dB can be built, kept and played at up to +1240 dB
  while `Preamp` reaches -40 dB alone (`fuzz/seeds/equaliser/stacked-boosts.txt`). Guarding the read
  alone would make a profile the editor allowed unreadable: bound the editor and `keep` too, or
  read the limit in `apo::read`, or widen the preamp. Done when no path keeps or plays a lift the
  preamp cannot take back, and a kept profile past it still loads (as a migration would).
- [S · 0.18.1] A shelf above 20 kHz at a hi-res rate passes full scale after *Fit the preamp*.
  **Low, builder.** `fitted_preamp` weighs 20 Hz to 20 kHz alone (`RESPONSE_TO_HZ`), so a high
  shelf of +24 dB at 20 kHz played at 192 kHz lifts everything above it unweighed: an impulse
  peaked at 2.25 (found by the `equaliser` fuzz target, whose bound now reads to Nyquist). Weigh the
  response to the stream's Nyquist, or say why the band past hearing is left. Done when the fitted
  preamp holds that profile's impulse under full scale, in a test.

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
- [S · 0.18.1] A history import says *1 were held already*: its count of listens kept before is
  not a Fluent selector (`crates/resonate/words/en/main.ftl`, the `history` messages). Select on
  the count in every language. Done when one held listen is said in the singular, in a test.
