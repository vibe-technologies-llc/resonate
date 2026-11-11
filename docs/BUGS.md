# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Security

## Sync and together

## Playback and output

## Catalog
- [S · 0.8.1] Tidying the gone and a scan's tidy delete listens without keeping them: `forget_paths`
  (`forget_the_gone`, the scan's tidy) deletes the rows' listens with them and makes no
  `unheld_listens` copy as `remove_root` (`deleted::forget_a_root`) and `deleted.rs` do, so a file
  moved off a mount and back is counted from nothing and Statistics, unfrozen Wrapped years and
  scrobbles not yet sent lose it
- [S · 0.8.1] Online off does not stop a build of the local MusicBrainz copy: the binary's
  `musicbrainz::build` asks `online_enabled` once before a 7.5 GB download, `Mirroring` holds the
  config cloned as the window opened (so *Build it now* reads Online as it stood at start),
  `online::reach(false)` and `follow(false)` cancel nothing, a build landing after the mirror was
  switched off holds it again from its own forced config, and `keep_fresh` asks `LATEST` past the
  metered lookup gate; `online.md` has `online = false` stop every request. `musicbrainz.rs` holds
  no test
- [S · 0.8.1] A resumed download of the MusicBrainz export shows more read than there is:
  `download.rs` takes the total from `source.length()`, which on a `206` is the bytes still to
  come, while `written` starts at what the part held, so the settings pane reads past 100 %; the
  total is `source.total()`, else the length and what was held
- [S · blocked: a Linux unsafe exception] Lengthening a file in place still cannot see a reader
  root runs: `held_open_elsewhere` (`insert.rs`) counts an unreadable `/proc/<pid>/fd` as holding
  the file except for a process whose `status` names root. A write lease (`F_SETLEASE` with
  `F_WRLCK`, then `F_GETLEASE` after the insert to see whether a break released it) would see it, but
  neither `rustix` nor `nix` wraps leases and the codec forbids `unsafe`, so it wants a small crate
  under an exception of its own beside `resonate-win32` and `resonate-apple`, which is the
  listener's to grant

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

## The window
- [S · 0.8.1] Late network answers act on a window that has moved on: *Find another version*
  opens its menu at the press's place whenever the services answer (`better.rs`), as *Place on a
  release…* does (`browser.rs`), and `open_a_menu` clears `naming`, `adding` and `record`, so an
  answer landing while a playlist is named or picked drops it and its held rows; an album or artist
  not held opens its page whenever its lookup lands (`open_album_not_held`, `root.rs`), so of two
  pressed, whichever answers last is shown. A serial taken at the press and moved by every pane,
  sheet and lookup
- [S · 0.8.1] The Match sheet takes an answer asked by an earlier opening: `ask_to_match` starts
  `asked` at 0 each time the sheet opens (`matching.rs`) and a landing compares the track and that
  count alone, so a sheet reopened on the same song before MusicBrainz answers takes the first ask's
  recordings (for the words typed then), and they overwrite the newer where they land after
- [S · 0.8.1] A remote zone's clock shows no hours: `remote.rs`'s `clock` prints `75:03` where the
  Device sync pane prints `1:15:03` for the same zone. The window, the panes and the terminal hold
  some eight clock formatters of their own (`format.rs`'s `whole_seconds` and `spanned`, the
  binary's `played`, `readout.rs`, `sync.rs`, `settings/sync.rs`, `analysis_plot.rs`), and the
  terminal's `ago` stops at days (*400 days ago*) where the window says years (`format.rs`'s
  `since`); one formatter in core would serve both
- [S · 0.8.1] Forgetting the served covers lets fetches already running land: `serving.rs`'s
  `forget_the_covers` clears `fetching` and the cache while detached fetches run, and each landing
  removes a key a newer fetch may hold and keeps a cover from before the reset
