# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Audio
- [S · 0.17.0] The true-peak guard lets an inter-sample over out 0.09 dB past its ceiling: a sine at
  a quarter of the rate, 45 degrees off its samples, with samples at 0 dBFS (+3.1 dBTP) leaves at
  −0.01 dBTP where the ceiling is −0.1 (`TruePeakMeter`, at 44.1, 48 and 96 kHz), while a +1 dBFS
  997 Hz tone is held at −0.10 exactly; `dsp-lab measure peak` (`docs/DSP.md`) reads it

## Android
- [S · blocked: design] The widget's previous and next do nothing while the playback service is not
  running: play starts the service as the headset's key does (`WidgetPresses`), but a skip that
  starts nothing cannot meet the foreground-service rule, and Media3's own `MediaButtonReceiver`
  passes over every key but play cold; whether a cold skip should start playing the row before or
  after is the open choice

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

## Formats
- [M · blocked: symphonia] A file embedding a huge picture still costs one materialisation:
  symphonia 0.6.1 reads it into its own buffer before `probe_cover_art` can weigh it; its
  `limit_visual_bytes` is honoured by no reader
- [M · blocked: opus-rs] A stereo Opus stream's mono packets (libopus drops to mono near 16-24 kbps)
  go to a second `OpusDecoder` (`opus.rs` `other`) whose state is minutes stale or empty, and the
  main decoder misses them: a click, and band energies mispredicted for some frames, at every
  mono/stereo switch of a low-bitrate podcast; `opus-rs` 0.1.37 refuses a packet whose TOC channel
  bit differs from the decoder's (`Channel count mismatch`) and keeps no stream-channel state a
  mono packet could be decoded against, so the fix is a decoder that switches (upstream or vendored)

## macOS

