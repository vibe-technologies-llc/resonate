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
- [S · 0.14.1] One synced change stamped near `i64::MAX` freezes the sync ledger's clock for good:
  `ledger.rs` calls `Clock::saw` on every change a paired device sends and saves that clock, so
  `now()` stays at `i64::MAX` and every later change of a key compares equal to the held one and is
  dropped; a stamp too far past the wall clock should be refused as shared playlists now refuse one
  (`STAMP_AHEAD_OF_THE_WALL_AT_MOST_NANOS`)


## Catalog
- [S · 0.14.1] Organising a second name of a filed track, a download client's hard link in another
  folder, renames it in its own folder rather than filing it: `as_the_volume_names_it`
  (`organise.rs`) sees the same file at the destination and narrows the landing to the link's own
  folder, meant for a case-only rename on a case-insensitive volume, so `downloads/echoes.wav`
  becomes `downloads/02 Echoes.wav` and the two names stay apart
- [M · 0.14.1] A phone's batch of moved documents that lands while a zone opens a row drops the
  moved songs from that zone: `documents_moved` (`organising.rs`) leaves such a zone's live queue
  on the old URIs and rekeys the catalog, so each moved song fails when reached and the next
  `save_zone` (`ids_held_at`) leaves it out of the saved queue for good
- [M · blocked: platform] On macOS and Windows a retag that grows a hard-linked track's head while it
  plays still shifts the bytes under the decoder: neither can tell who holds a file
  (`Shifting::readers_can_be_asked` is false), so the copy is written back over the same inode as
  before rather than refusing every such write; Linux and Android wait for the readers under a lease

## Providers and network
- [S · 0.14.1] A server answering an open range with a capped `206` ends the song early: `Ranged`
  (`resonate-fetch`'s `ranged.rs`) checks only where `Content-Range` starts and returns the body's
  `Ok(0)` as the end though it holds `total`, `Seeking` passes it up, and `resonate-upnp`'s
  `Fetched` frames the slice by its length and fails the seek; a Subsonic, Monochrome or hifi-api
  song played in place stops short, and a resumed download lands short and is filed `NotTheSong`,
  refusing the offer for 30 days; a read ending under `total` should ask again from where it is
- [S · 0.14.1] A seek whose skipping copy fails part way leaves `Seeking` reading from another place:
  `passed_over` (`seeking.rs`) returns on the error before `self.at` moves while `self.reading`
  stays set, so a later read with no seek between hands back bytes from an offset `at` does not
  name
- [S · 0.14.1] Recognising a 705.6 or 768 kHz file never asks Shazam: the signature's resampling to
  16 kHz (`signature.rs`) is refused for a ratio under 1/32 and answers an empty signal, and
  `Shazam::signed` logs too few peaks and gives up with no error

## The window
- [S · 0.14.1] A song fetched to play lands on the zone shown when the download arrives, not the one
  it was asked on: `fetch_and_play` waits for the library model's arrival and then plays through the
  zone shown, so turning to another zone while it downloads replaces that zone's queue; the zone
  should travel with the download as `act_on_the_zone` carries it through a catalog read

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

## Windows
- [S · 0.14.1] A desktop recording does not follow a change of the default output: `opened_for`
  (`wasapi_capture.rs`) binds loopback to the default render endpoint once and the loop watches for
  no change, `lost` being set only by a failing call, so after a headset becomes the default the old
  endpoint delivers nothing, the clip is padded with silence and recognition fails; `windows.md`
  says a changed default is a loss that reopens the capture
- [S · 0.14.1] A file URI spelt with a lowercase drive letter (`file:///c:/Music/a.flac`, VS Code's
  `file:///c%3A/…`) finds no catalog row: `path_of_uri_rest` and `verbatim` (`windows_uri.rs`) keep
  the spelling, while the scan stores `fs::canonicalize`'s, so an imported playlist's song plays but
  is tied to no row, against `windows.md`'s claim that `from_uri` yields the scan's spelling
- [S · 0.14.1] The WASAPI render loop never sees a stream broken while its event stays quiet:
  `serving.rs` skips `cycle()` whenever `wait_for_room` times out, and failure is learnt only there,
  so a chosen endpoint unplugged or reformatted that stops signalling leaves the zone playing with
  its position frozen and a drain unsettled, where the capture loop calls into the client every
  turn; a timeout while streaming should probe `GetCurrentPadding`

## macOS
- [S · 0.14.1] The window never offers *Desktop* on a Mac though the process tap records it:
  `DESKTOP_IS_HEARD` (`hearing.rs`) is still `cfg!(not(target_os = "macos"))` from before the tap,
  so the Listen pane draws only *Microphone* and the default stays `Microphone(None)`; only
  `listen_from = "desktop"` in the file reaches what `macos.md` describes

