# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Android
- [S · blocked: design] The widget's previous and next do nothing while the playback service is not
  running: play starts the service as the headset's key does (`WidgetPresses`), but a skip that
  starts nothing cannot meet the foreground-service rule, and Media3's own `MediaButtonReceiver`
  passes over every key but play cold; whether a cold skip should start playing the row before or
  after is the open choice

## iOS
- [M · 0.14.1] The iPhone's session gives up Now Playing while another zone plays: it follows the
  zone shown alone, so showing a stopped or empty zone drops the lock-screen controls and an
  interruption or a route lost pauses only the shown zone, while a zone on another output plays on;
  the boundary's snapshot now carries `heard_elsewhere` and `pause_sounding_zones`,
  `play_zones` and `stop_every_zone`, which Android's session uses (`android.md`)

## Security
- [M · blocked: design] A device that joins a shared playlist after a hostile editor compacted it
  takes the forged member keys from the snapshot: the first key heard for a `MemberId` stands, so
  devices that already held the list keep the true keys, but a newcomer's first is the forger's, and
  a removal it later makes seals the other members' seats to them; closing it needs `Member` ops
  signed by the member's own key, a change of the op format
- [S · blocked: design] A host on the network may take several addresses and so pass the media server's
  per-neighbour share of its answers (`door.rs`); nothing on a LAN names a host better than its address

## Sync and together
- [S · 0.14.1] A song whose file moves while a party is followed is lost to the party: a guest's rows
  are known by a `QueueStamp` of their location, so on the desktop a `Command::Relocate` (organise,
  which reaches every zone) makes the moved row read as a song added here, and `added_here` takes it
  out and asks the host for it; on the phones `documents_moved` leaves the following zone on the old
  URIs instead, so the song fails when reached; the seat's known stamps should follow the move
- [S · 0.14.1] A synced row a paired device stamped near `i64::MAX` before the ledger refused such
  stamps still wins every later write of its key: the ledger now refuses a stamp more than a day
  past the wall clock and sets a saved clock back on load (`Clock::from_saved`), but rows already
  stored keep their stamp, so an honest edit of that key never lands; such rows should be restamped
  to the wall clock when the ledger opens


## Catalog
- [M · blocked: platform] On macOS and Windows a retag that grows a hard-linked track's head while it
  plays still shifts the bytes under the decoder: neither can tell who holds a file
  (`Shifting::readers_can_be_asked` is false), so the copy is written back over the same inode as
  before rather than refusing every such write; Linux and Android wait for the readers under a lease



## Formats
- [S · 0.14.1] The vault keeps a 64-bit float master at 32-bit float and calls it bit-exact:
  `representable` (`container.rs`) opens `F64` as `F32`, `Form::of` sends a float to `Form::Wave`,
  `kept_as_wave` writes `F32`, and the read-back compares `F32` with the `F32` tally, so it always
  passes, while `vault_bits` still says 64; once the source is deleted the vault holds the only copy,
  narrowed; an `F64` source should be `Form::Kept`
- [M · 0.14.1] The vault narrows a FLAC to the depth the container declares without checking that
  only zero bits go: `bits_per_coded_sample` (a WAVE's valid bits, Matroska's `BitDepth`) picks S16
  or S24 (`flac_depth`), the decoder shifts the low byte away, and the key and read-back digest the
  narrowed samples, so a 32-bit file declaring 24 valid bits with live low bits lands lossy as
  validated
- [S · 0.14.1] An embedded picture whose declared type is wrong (PNG bytes labelled `image/jpeg`)
  never decodes: `decode_visual` and the attachments path (`probe.rs`) take the format from
  `media_type` and sniff only when none is declared, `decode_cover` and the vault's `cover.rs` force
  it, so the window and the phones draw no cover and the vault passes it over
- [S · 0.14.1] A leading ID3 tag over 32 MiB moved into a WAVE can no longer be read: `chunked`
  (`writing.rs`) moves the whole tag into an `id3 ` chunk, which `read_id3_chunk` (`riff.rs`) skips
  past `MAX_ID3_CHUNK_BYTES`, so the write reads back unconfirmed and every scan after reads the file
  untagged
- [S · 0.14.1] Cue tracks sharing an `INDEX 01` become a row that plays nothing: `close` (`cue.rs`)
  drops a track only when it starts strictly before the one before it, so `span_of` builds a span
  of zero frames
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

