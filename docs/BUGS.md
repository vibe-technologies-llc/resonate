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
- [S · 0.17.1] Show a definite Android remote-watch failure when sync stops.
  **Low, builder.** The drive panel drops read failures and displays its
  waiting state even after a device is removed or sync turns off
  (`android/app/src/main/java/org/resonate/android/ui/settings/DrivenDevice.kt:154`,
  `crates/resonate-mobile/src/sync/driving.rs:377`). Preserve the typed outcome
  and retire the watch for a known unavailable device. Done when those
  transitions replace the waiting state and a transient read can still recover.
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

