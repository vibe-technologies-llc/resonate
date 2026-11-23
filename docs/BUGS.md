# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Vault
- [M · 0.17.1] Verify a kept object before reusing it. **High, architect.** A second import of a
  track can discard a verified staged copy and keep a damaged archive object of the same length
  (`crates/resonate-vault/src/vault.rs:1478`, `crates/resonate-vault/src/vault.rs:1651`).
  Reuse the byte-digest check from `verify` on the deduplication path and keep the verified copy
  when the digest differs. Done when re-importing against a same-length damaged object restores
  the expected bytes and the catalog points to an object that passes verification.
- [M · 0.17.1] Check a stored cover before counting it as a deduplication hit. **Medium, architect.**
  `crates/resonate-vault/src/vault.rs:676` accepts the dimensions from the stored header as proof
  that the whole image still reads. Verify the stored form against the incoming cover before reuse.
  Done when re-import repairs a cover with a readable header and damaged image data and still
  deduplicates all intact stored forms.

## Playback and output
- [M · 0.17.1] Finish graph recovery when its deadline expires. **High, architect.**
  If the backend stays disconnected, the engine clears the wait and sends the timeout to
  `fail`, which treats the same disconnection as a fresh loss and starts another ten-second wait
  (`crates/resonate-engine/src/engine/graph.rs:78`, `crates/resonate-engine/src/engine/graph.rs:152`,
  `crates/resonate-engine/src/engine/graph.rs:375`). Carry the exhausted recovery state into
  failure handling. Done when a fake backend that stays away emits a row failure within one
  recovery window and a backend that comes back in that window still resumes the row.
- [M · 0.17.1] Move out of Playing when a rebind retires the old output and fails.
  **High, architect.** A device refusal during a settings rebind can leave the active row with
  no output and no recovery scheduled. Tick-driven rebinds can leave the same state by logging
  the error and returning (`crates/resonate-engine/src/engine/binding.rs:110`,
  `crates/resonate-engine/src/engine/binding.rs:256`,
  `crates/resonate-engine/src/engine/commands.rs:149`). Route stranded rows through the existing
  device wait or row failure handling. Done when a settings rebind and an automatic rebind that
  lose their output each resume on a recovered device or publish a terminal state.
- [S · 0.17.1] Retire a disconnected sink survey receiver from the engine's wait set.
  **Low, builder.** In an unwinding build a stopped survey worker leaves the engine polling a
  disconnected receiver as if it were ready, consuming CPU until another receiver changes
  (`crates/resonate-engine/src/engine.rs:686`, `crates/resonate-engine/src/engine.rs:821`,
  `crates/resonate-engine/src/surveying.rs:75`). Include survey receiver changes in the wait set
  comparison. Done when a test with a disconnected survey worker observes the wait set rebuilt and
  the engine still answers transport commands.
- [M · 0.17.1] Honor the initial spool deadline during a remote read. **Low, architect.**
  A single slow read can stall beyond the 750 ms initial spool budget because elapsed time is
  checked between reads and the source's read timeout defaults to five seconds
  (`crates/resonate-codec/src/container.rs:508`, `crates/resonate-codec/src/source.rs:215`).
  Use the remaining spool budget while waiting and carry the in-flight read into the next phase.
  Done when a gated non-seekable source hands off at the spool deadline and every byte is still
  delivered once the stalled read completes.
- [S · 0.17.1] Measure free space on the volume holding the Windows spool. **Low, builder.**
  A caller supplying a spool folder on another volume gets the limit for the default temporary
  drive (`crates/resonate-codec/src/spool.rs:98`, `crates/resonate-codec/src/spool.rs:242`).
  Query the volume of the file or the chosen folder. Done when a Windows test with distinct
  reported capacity for the two locations uses the chosen location for admission.

## Security
- [M · blocked: design] A device that joins a shared playlist after a hostile editor compacted it
  takes the forged member keys from the snapshot: the first key heard for a `MemberId` stands, so
  devices that already held the list keep the true keys, but a newcomer's first is the forger's, and
  a removal it later makes seals the other members' seats to them; closing it needs `Member` ops
  signed by the member's own key, a change of the op format
- [S · blocked: design] A host on the network may take several addresses and so pass the media server's
  per-neighbour share of its answers (`door.rs`); nothing on a LAN names a host better than its address


## Catalog
- [L · 0.17.1] Preserve a moved track across an interrupted scan. **High, architect.**
  New rows are committed before move reconciliation. If a scan stops there, the next scan
  excludes the arrival from pairing because it was added in an earlier generation, then
  prunes the old row. The file survives with a fresh ID and loses its favourite, play count
  and saved queue references (`crates/resonate-library/src/scan.rs:640`, `:666`,
  `crates/resonate-library/src/moves.rs:280`, `:307`). Keep arrivals eligible until
  reconciliation has settled them. Done when a scan interrupted after committing the
  arrival and a destination-only scan followed by a full scan both preserve the old ID,
  favourite, listens and queue location, including for cue cuts.
- [M · 0.17.1] Preserve another writer's tag recovery files on a name collision. **High, architect.**
  A failed exclusive journal create unlinks the existing journal, and whole-copy landing can
  overwrite a name another writer holds (`crates/resonate-codec/src/journal.rs:131`,
  `crates/resonate-codec/src/journal.rs:322`, `crates/resonate-codec/src/journal.rs:350`).
  Keep the existing file on collision and obtain a fresh name or refuse the write. Done when
  forcing each collision leaves the original recovery record intact and available to recovery.
- [M · 0.17.1] Keep organise's filename swaps together when a move fails or is cancelled.
  **Medium, architect.** The park and landing are ordinary moves, so cancellation or an I/O refusal
  can commit only the park and leave the catalog pointing at a temporary filename
  (`crates/resonate-library/src/organise.rs:2104`, `crates/resonate-library/src/organise.rs:2300`,
  `crates/resonate-library/src/organise.rs:2384`). Apply the entire cycle as one unit and roll back
  an incomplete one. Done when a cancelled swap, an injected landing refusal and a swap at a
  batch boundary leave both file paths and catalog entries consistent.
- [S · 0.17.1] Keep a fractional APE rating during a play-count-only update. **Medium, builder.**
  A retag that changes the count for an unfavourited track removes its existing fractional
  `FMPS_RATING` (`crates/resonate-codec/src/counted.rs:168`,
  `crates/resonate-codec/src/counted.rs:180`, `crates/resonate-codec/src/writing.rs:1069`).
  Update the rating only when the favourite state changes. Done when a WavPack or Monkey's Audio
  fixture retains its fractional rating after a count update and still supports adding and removing
  a favourite.
- [M · 0.17.1] Reconcile both roots when the file watcher reports a split rename.
  **Medium, architect.** Windows and macOS can report the old and new paths separately.
  The old path goes straight to deferred removal and only the new root is scanned, so the
  old row can be forgotten without transferring its state
  (`crates/resonate-library/src/watch.rs:172`, `:225`,
  `crates/resonate-ui/src/models/watching.rs:112`). Queue both roots and defer the old
  path's removal until the move has been checked. Done when synthetic watcher events for
  a split rename across roots preserve the original track ID and its favourite.
- [M · 0.17.1] Record newly discovered volumes with the tracks from the first scan.
  **Medium, architect.** Track batches can commit before the scan records their mount
  point. If the first pass stops and the drive is then unplugged, a remaining empty
  mount directory looks like an ordinary folder and the next scan prunes its rows
  (`crates/resonate-library/src/scan.rs:640`, `:692`, `:1011`,
  `crates/resonate-library/src/volumes.rs:195`). Persist mount information before or
  with the first track batch. Done when cancellation and write failure before final
  settlement leave a root on a now-unmounted volume with its IDs and favourites intact.
- [M · 0.17.1] Keep catalog rows when a delete reaches an offline volume.
  **Medium, architect.** A missing mount path is treated as a missing file, so deletion
  can discard catalog state and mark a recording deleted while its file is still on disk
  (`crates/resonate-library/src/deleted.rs:108`,
  `crates/resonate-library/src/db/roots.rs:195`, `:219`). Use the catalog's mounted-volume
  check and require positive evidence of absence from a reachable volume before settling
  a deletion. Done when a delete on an unplugged root or a refused stat preserves its
  track and local deletion state, and a confirmed missing file is handled normally.
- [M · 0.17.1] Recover desktop deletion state after a partial trash pass. **Medium, architect.**
  Local deletion marks commit before the files move, then the catalog and outbound
  every-device notices settle in a later transaction. An interruption between them can
  leave a present file marked deleted or a trashed file with no notice for peers
  (`crates/resonate-library/src/db/roots.rs:179`, `:213`,
  `crates/resonate-library/src/deleted.rs:206`, `:242`). Persist the pending intent and
  reconcile what actually reached the trash on restart. Done when failure tests before
  and after a simulated move recover rows, local marks and pending notices exactly once.
- [M · 0.17.1] Recheck identical copies before folding them. **Medium, architect.**
  Apply uses the preview's rows and paths. If the listener edits or moves a file in
  another app, the fold can transfer history and queue references to a different or
  unavailable file and trash the remaining copy (`crates/resonate-library/src/db/roots.rs:109`,
  `crates/resonate-library/src/copies.rs:124`,
  `crates/resonate-ui/src/models/filing.rs:299`). Resolve both rows and validate the files
  under the walk guard before changing either. Done when modifying either file or removing
  a previewed row makes that pair unavailable to apply and leaves unrelated pairs usable.
- [M · 0.17.1] Claim each deferred catalog repair in its own transaction. **Medium, builder.**
  Two processes can both read a pending marker before either obtains the write lock.
  The next opener retries the repair against an already repaired catalog and fails when
  dropping the missing marker (`crates/resonate-library/src/store.rs:277`, `:330`,
  `:868`, `crates/resonate-library/src/db/mod.rs:1167`). Read the marker and any merge inputs
  inside the same immediate transaction as the repair. Done when simultaneous opens on a
  file-backed catalog with pending repairs both succeed and apply each repair once.
- [M · 0.17.1] Credit kept listens after a scan stops partway through. **Medium, builder.**
  A batch can commit a newly catalogued track before the scan reaches the listen credit.
  A follow-up scan that adds no fresh tracks skips the credit again
  (`crates/resonate-library/src/scan.rs:640`, `:693`,
  `crates/resonate-library/src/history.rs:174`). Let a completed pass credit matching
  retained listens regardless of how many files it just added. Done when a committed
  arrival followed by a cancelled pass and an unchanged rescan restores every listen
  and the play count exactly once.
- [M · 0.17.1] Export listening history from one catalog snapshot. **Low, architect.**
  The exporter pages through held and then unheld listens in separate reads.
  A scan crediting a retained listen between reads can omit it from the export, and
  a removal moving a held listen in the other direction can include it twice
  (`crates/resonate-library/src/kept_listens.rs:34`,
  `crates/resonate-library/src/traded.rs:111`, `:151`). Hold one read snapshot for the
  export. Done when credits and removals committed between pages leave the export with
  every listen present at its start exactly once.
- [S · 0.17.1] Refresh advisory and frame metadata when following a move. **Low, builder.**
  A move keeps the old row and replaces its file stamp but leaves `explicit` and
  `walked` untouched. A simultaneous retag or container rewrite can leave stale data
  with a current stamp (`crates/resonate-library/src/moves.rs:650`,
  `crates/resonate-library/src/store.rs:2275`). Copy both values from the newly probed
  row. Done when a followed move keeps the original ID and reflects the arrival's
  advisory and cached frame count.
- [M · 0.17.1] Scan each root after file monitoring recovers. **Low, builder.**
  A root that regains its watch starts receiving new events, but changes made while it
  was uncovered remain invisible. Replacing the watcher has a similar gap
  (`crates/resonate-library/src/watch.rs:111`, `:128`,
  `crates/resonate-ui/src/models/watched.rs:71`). Queue one catch-up scan after
  re-establishing coverage. Done when a file changed during an uncovered interval is
  picked up after coverage returns without needing another filesystem event.
- [S · 0.17.1] Release the catalog's file lock before a walk reports completion.
  **Low, builder.** A waiting operation can observe that the walk ended while the old
  guard still holds the lock and be refused as already walking
  (`crates/resonate-library/src/db/mod.rs:244`, `:394`). Close the file guard before
  clearing the flag and notifying. Done when a caller woken by `wind_down` can take the
  lock in a file-backed catalog with no other walkers.
- [M · 0.17.1] Keep destination files when a move lacks an atomic no-replace operation.
  **Low, architect.** The fallback can use a replacing rename after a separate existence
  check, so a concurrently created destination file can be lost
  (`crates/resonate-library/src/unreplacing.rs:23`). Use a platform operation that
  reserves the destination atomically or refuse the move on that volume. Done when a
  fallback test preserves both the source and a competing destination.
- [M · 0.17.1] Report sidecars left behind by a refused trash move. **Low, builder.**
  The audio file can reach the trash while a sidecar remains, and the completed result
  reports only the audio move (`crates/resonate-library/src/db/roots.rs:196`,
  `crates/resonate-library/src/deleted.rs:133`). Surface the partial outcome while
  preserving shared sidecars. Done when a trash stub that accepts audio and refuses its
  lyrics leaves the lyrics in place and reports them as kept.
- [M · blocked: platform] On macOS and Windows a retag that grows a hard-linked track's head while it
  plays still shifts the bytes under the decoder: neither can tell who holds a file
  (`Shifting::readers_can_be_asked` is false), so the copy is written back over the same inode as
  before rather than refusing every such write; Linux and Android wait for the readers under a lease

## Mobile catalog
- [M · 0.17.1] Recover the original after an interrupted Android tag rewrite. **High, architect.**
  The document provider can truncate a song before the replacement bytes land. If the
  app stops during that write, the next tag edit can sweep the only intact copy
  (`android/app/src/main/java/org/resonate/android/TagEditing.kt:183`,
  `crates/resonate-mobile/src/rewriting.rs:137`, `:166`,
  `crates/resonate-mobile/src/tagging.rs:312`, `:340`). Sync the original and record the
  pending rewrite before opening the destination, with the source already readable.
  Retain the original until success or
  restoration is durable, including when moving the backup aside fails. Done when an
  interrupted write followed by a new process, and a restore failure followed by a
  failed backup move, both leave a verified copy recoverable.
- [L · 0.17.1] Recheck the file a phone will keep before deleting identical copies.
  **High, architect.** A saved preview can outlive the kept file or its contents.
  Android deletes the other files directly from that preview. iOS checks for a matching
  file identity, but a failed lookup still lets deletion proceed
  (`android/app/src/main/java/org/resonate/android/SongDeletion.kt:60`,
  `crates/resonate-mobile/src/deleting.rs:172`,
  `ios/Sources/Resonate/Deleting.swift:65`, `ios/Sources/Resonate/Folders.swift:415`).
  Require a readable kept file and verify distinct identity and matching content
  before each removal. Done when both a stale preview and two document aliases that
  cannot be proven distinct leave the files intact.
- [M · 0.17.1] Keep full study results when opening the phone's Analyse pane. **Medium, builder.**
  The pane computes a study, then persists a summary that clears the algorithm version,
  findings and fingerprint from a previously studied song
  (`crates/resonate-mobile/src/features.rs:543`, `:566`,
  `crates/resonate-mobile/src/catalog_extras.rs:71`, `:457`). Persist the full study or
  let the summary update only the fields it owns. Done when opening the pane on an
  already measured song keeps its recognition data and it remains eligible to share.
- [L · 0.17.1] Keep mobile queue entries when a document provider is temporarily unavailable.
  **Medium, architect.** A folder can pass a readability check before a document open
  fails. The iOS bookmark-open path and Android's raw-descriptor path can then report
  not-found without establishing the document is missing. The catalog hides it and
  removes it from the queue (`ios/Sources/Resonate/Folders.swift:156`,
  `android/app/src/main/java/org/resonate/android/LibraryRepository.kt:290`,
  `crates/resonate-mobile/src/vanished.rs:63`). Preserve the row after an ambiguous
  open failure. Done when bookmark loss and an unavailable subfolder leave rows and
  queue entries intact, and confirmed absence still hides the song.
- [M · 0.17.1] Require a confirmed MediaStore result before settling a delete.
  **Medium, builder.** After a delete returns zero, a denied query or null cursor
  reads the same as an empty result. The copy-fold path then removes the catalog and
  queue entries for a file that may still exist
  (`android/app/src/main/java/org/resonate/android/MusicDocuments.kt:189`, `:202`,
  `crates/resonate-mobile/src/deleting.rs:112`). Distinguish unavailable from absent
  and keep the row when the outcome is unknown. Done when a provider returning zero
  and then a null cursor or a permission error leaves the copy in the catalog.
- [M · 0.17.1] Preserve saved phone queues until startup restoration publishes them.
  **Medium, architect.** Opening a player sends Resume asynchronously. A save or
  shutdown before the engine publishes that command can replace a saved queue with
  the fresh engine's empty queue (`crates/resonate-mobile/src/lib.rs:434`, `:770`,
  `:850`, `crates/resonate-mobile/src/zoning.rs:696`). Wait for the restore to settle
  or defer saving that zone until its queue is authoritative. Done when a file-backed
  open with an intentionally delayed Resume leaves the first and secondary queues
  intact across an immediate save and shutdown.
- [M · 0.17.1] Keep a phone document's analysis when reopening an unchanged file.
  **Low, builder.** Android's open-with path can reindex a known document without a
  size or modification stamp. That clears the old stamp and triggers a reset of its
  cached analysis (`android/app/src/main/java/org/resonate/android/OpenedFiles.kt:186`,
  `crates/resonate-mobile/src/catalog_extras.rs:190`,
  `crates/resonate-mobile/src/catalog.rs:138`). Preserve known stamps when a read provides
  no newer one, while still invalidating analysis on confirmed changes. Done when indexing
  a stamped file from open-with keeps the analysis and scanning a changed file refreshes it.
- [M · 0.17.1] Save phone queue edits while a document is still opening. **Low, architect.**
  A buffering or paused row with no opened track suppresses all queue persistence,
  including the last save at shutdown (`crates/resonate-mobile/src/lib.rs:251`, `:744`,
  `:770`). Persist edits and the selected row while deferring the unresolved play
  position. Done when a blocked open followed by shutdown preserves the changed
  queue and reopens at the intended song without overwriting an older pending seek.
- [M · 0.17.1] Save phone zones independently and keep removed zones removed. **Low, architect.**
  A zone removed after the save enumerates it can make the save skip every later zone.
  An in-flight volume upsert can also recreate the removed row
  (`crates/resonate-mobile/src/lib.rs:763`,
  `crates/resonate-mobile/src/zoning.rs:435`, `:530`, `:718`). Coordinate removal with
  persistence and continue saving other zones after an error. Done when an interleaved
  removal and save leave the deleted zone absent on disk and preserve the other queues.
- [S · 0.17.1] Keep shuffle enabled when adding to an empty phone queue. **Low, builder.**
  The empty-queue insertion builds a default resumption that disables the user's shuffle
  setting (`crates/resonate-mobile/src/queue_edit.rs:57`, `:122`). Carry the existing
  setting through the insertion. Done when Add to Queue and Play Next populate an empty
  queue without toggling shuffle off.

## Formats
- [M · 0.17.1] Read Matroska bit depth from the selected audio track. **Medium, builder.**
  For streams without a coded depth supplied by the decoder, the prescan can lose the declaration
  or supply a different track's bit depth (`crates/resonate-codec/src/matroska.rs:1047`,
  `crates/resonate-codec/src/container.rs:561`, `crates/resonate-codec/src/container.rs:918`).
  Match the container track entry to the audio track chosen for playback. Done when fixture files
  with a preceding video track or a different default audio track use the selected audio depth.
- [S · 0.17.1] Recheck cached frame counts when an equal-size file has changed.
  **Medium, builder.** The catalog supplies the old walk and byte length, and the prescan
  treats an equal-length replacement as unchanged
  (`crates/resonate-library/src/hinted.rs:24`, `crates/resonate-codec/src/prescan.rs:93`).
  Compare the file's live modification stamp with the catalog's stamp before reusing its
  MPEG frame count or Matroska cluster tally. Done when an equal-length replacement is
  walked again, and an unchanged track still uses its cached walk.
- [M · 0.17.1] Make a combined Matroska tag and cover write's result explicit. **Low, architect.**
  Tag edits can land before cover attachment is refused, while the tag writer returns one error
  and the rules promise the file was untouched (`crates/resonate-codec/src/writing.rs:759`,
  `crates/resonate-codec/src/matroska_tags.rs:1124`, `.claude/rules/library.md:1582`).
  Give the combined operation one commit, or return a typed partial result that the delivery path
  can handle. Done when a fixture with a cover the writer must refuse proves the documented
  outcome and a filed delivery still gets its text tags.
- [S · 0.17.1] Bound DSDIFF duration when declared DST frames cannot fit in the data chunk.
  **Low, builder.** The container publishes the declared count as duration and seek range even if
  the available stream is much shorter (`crates/resonate-codec/src/dsd/dff.rs:147`,
  `crates/resonate-codec/src/dsd/dst.rs:499`). Reject or reconcile a count incompatible with the
  stored frames. Done when a fixture with an overstated count cannot publish an impossible seek
  range and a well-formed file keeps its sample count.
- [M · blocked: symphonia] A file embedding a huge picture still costs one materialisation:
  symphonia 0.6.1 reads it into its own buffer before `probe_cover_art` can weigh it; its
  `limit_visual_bytes` is honoured by no reader

## macOS

