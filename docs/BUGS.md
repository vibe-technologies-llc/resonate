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


## Playback and output
- [S · 0.18.1] Hold a true-peak stage's flush to once. **Low, builder.** After `TruePeak::flush`,
  `frames_held` still reports its `delay` and a second flush emits `delay` frames more
  (`crates/resonate-dsp/src/`), so the contract test (`crates/resonate-dsp/tests/contract.rs`)
  cannot ask every stage to hold nothing after a flush, as `Restore` does. Empty the delay line on
  flush. Done when the contract test asserts nothing held after a flush for every stage.

## Catalog
- [S · 0.18.1] Fold a recording key's artist and album as its title is folded. **Low, builder.**
  `crates/resonate-core/src/recorded.rs:30-38`, `:56-69` word-normalise the title but keep the
  artist's and album's punctuation and spacing, so one recording keys apart under `Guns N' Roses`
  and `Guns N Roses`. Fold all three alike. Done when both spellings key the same.
- [S · 0.18.1] Correct `forget_folder`'s description. **Low, builder.** MCP says listens and
  playlist rows go (`crates/resonate-mcp/src/tools.rs:401-402`), but `forgetting.rs:30` keeps
  listens and playlist entries survive. Say what is kept. Done when the description matches the
  tool's test.
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
