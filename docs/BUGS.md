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

## Device sync
- [M · 0.17.1] Detect a viewed shared playlist's rebuilt space even after its cursor catches up.
  **Medium, builder.** A view link's `View` is said at `PROTOCOL_VERSION`, which
  the server answers with no incarnation, so once an editor has remade an
  expired shared space and written past a follower's cursor, the follower skips
  the new space's first records (`crates/resonate-sync/src/shared/service.rs`
  `visited_at`, `crates/resonate-server/src/serve.rs` `view`). Take `View` at
  `INCARNATION_VERSION` on the server, telling the incarnation, and say it there
  with the fallback `Share` has. Done when a follower returning to a remade
  shared space reads it whole.
- [S · 0.17.1] Show a definite Android remote-watch failure when sync stops.
  **Low, builder.** The drive panel drops read failures and displays its
  waiting state even after a device is removed or sync turns off
  (`android/app/src/main/java/org/resonate/android/ui/settings/DrivenDevice.kt:154`,
  `crates/resonate-mobile/src/sync/driving.rs:377`). Preserve the typed outcome
  and retire the watch for a known unavailable device. Done when those
  transitions replace the waiting state and a transient read can still recover.
- [S · 0.17.1] Turn the phones' MusicBrainz copy switch off when the copy is removed.
  **Low, builder.** Removing the copy now keeps it from being fetched again until
  `SyncConfig.musicbrainz_copy` is turned off and on, but Android and iOS leave their
  switch on, so it reads on while nothing is kept, and Android's line says it is looking
  (`android/app/src/main/java/org/resonate/android/LibraryFeatures.kt`,
  `android/app/src/main/java/org/resonate/android/ui/settings/SyncWords.kt`,
  `ios/Sources/Resonate/Features.swift`). Save the switch off after
  `removeMusicbrainzCopy`. Done when a removed copy shows the switch off and turning it
  on fetches the copy again on both phones.
## Catalog
- [M · blocked: platform] On macOS and Windows a retag that grows a hard-linked track's head while it
  plays still shifts the bytes under the decoder: neither can tell who holds a file
  (`Shifting::readers_can_be_asked` is false), so the copy is written back over the same inode as
  before rather than refusing every such write; Linux and Android wait for the readers under a lease

## Formats
- [M · blocked: symphonia] A file embedding a huge picture still costs one materialisation:
  symphonia 0.6.1 reads it into its own buffer before `probe_cover_art` can weigh it; its
  `limit_visual_bytes` is honoured by no reader

## macOS

