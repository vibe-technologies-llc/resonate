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


## Device sync
- [S · 0.17.1] Flush a MusicBrainz copy's partial before its fetch returns. **Low, builder.**
  A song transfer now syncs its partial to disc before the fetch answers, since the next attempt
  measured a partial whose last write had not landed and resumed from the wrong byte; the export
  transfer (`export_once`, `crates/resonate-sync/src/peer.rs`) still returns with its partial
  unflushed. Flush it the same way. Done when an interrupted copy transfer resumed at once lands
  bytes identical to the offered copy across repeated runs.
- [M · 0.17.1] Validate an Android join code before replacing the current group.
  **High, architect.** A nonblank manual entry can overwrite the stored pairing
  key. The native restart stops the old service before parsing the replacement,
  so a typo leaves sync stopped and the previous key can be lost
  (`android/app/src/main/java/org/resonate/android/ui/settings/SyncScreen.kt:396`, `:647`,
  `android/app/src/main/java/org/resonate/android/LibraryFeatures.kt:704`, `:740`,
  `crates/resonate-mobile/src/sync.rs:346`, `:357`, `:752`). Validate before saving
  or stopping the current service and preserve the old pairing on refusal.
  Done when an invalid manual code leaves the stored and active group usable,
  including after a relaunch.
- [M · 0.17.1] Keep the retry delay when a server link cannot apply its catch-up.
  **Medium, architect.** A catalog write refusal stops catch-up before the cursor advances.
  Every successful connection resets the delay even if the same record drops it again
  (`crates/resonate-sync/src/service.rs:1878`, `:1894`, `:2288`,
  `crates/resonate-mobile/src/sync.rs:1518`). Preserve backoff until catch-up makes progress
  and distinguish a local catalog refusal from a connection failure. Done when an injected
  write refusal backs off without losing the change and later records apply once the write works.
- [M · 0.17.1] Expire remote playback commands when their answers time out.
  **Medium, architect.** The request can report that a device did not answer, while the
  command stays queued and plays, pauses or skips when that device later reconnects
  (`crates/resonate-sync/src/service.rs:366`, `:2729`, `:3446`). Carry a deadline with the
  command and settle explicit transfer rows when their reachability attempts expire.
  Done when a device that returns after the deadline receives no expired command and
  a fetch to an unreachable device leaves Waiting.
- [S · 0.17.1] Retry an explicit fetch when a concurrent follow skips the song.
  **Medium, builder.** An automatic follow treats a deletion mark as if the song were already
  held and drops a concurrent explicit request that was waiting for the same fetch claim
  (`crates/resonate-sync/src/service.rs:3608`, `:3614`, `:3628`). Distinguish a skipped
  automatic fetch from a completed or already-held song. Done when an explicit request made
  during the deletion check is replayed and lands the song.
- [M · 0.17.1] Keep unread sync changes through a compaction.
  **Medium, architect.** A reader skips an answer kind it cannot read or advances the
  cursor after a bundle decode failure. Its snapshot contains only changes it interpreted,
  so compaction can discard a newer kind or readable neighbours of a malformed change
  (`crates/resonate-sync/src/wire.rs:982`, `:2350`,
  `crates/resonate-sync/src/service.rs:2288`, `:2560`,
  `crates/resonate-server/src/store.rs:790`). Preserve changes the snapshot cannot
  represent and apply independently framed changes that still read. Done when an older
  reader's compaction keeps a newer study answer available to a joining device and a
  malformed known change leaves its valid siblings recoverable.
- [M · 0.17.1] Keep address discovery off the sync command loop.
  **Medium, architect.** Each punch waits for the configured STUN name and replies in
  the loop that handles server frames, commands and Stop. Several waiting devices
  repeat the delay in series (`crates/resonate-sync/src/service.rs:1288`, `:1492`,
  `:2783`, `:2861`, `:2906`). Bound the lookup, share recent results across punches,
  and deliver the result without holding up the loop. Done when stalled resolution
  and a silent STUN server both leave status, commands and shutdown responsive.
- [M · 0.17.1] Try another sync server address when the first route stalls.
  **Medium, builder.** Secure WebSocket opens addresses in resolver order under
  one deadline. QUIC selects one address from the same result set. A silent route
  can leave either transport disconnected while a second address works
  (`crates/resonate-sync/src/web.rs:304`, `crates/resonate-sync/src/service.rs:1516`,
  `:1614`). Try addresses with bounded or staggered attempts and preserve TLS
  name checks. Done when both transports can use a working second route after
  the first stalls.
- [M · 0.17.1] Release a follow when a device stops listing its songs.
  **Medium, builder.** A holdings read waits indefinitely for its next frame
  while the connection remains open. The follow remains active, so later requests
  can only mark another follow due
  (`crates/resonate-sync/src/peer.rs:983`, `:994`,
  `crates/resonate-sync/src/service.rs:1975`, `:2736`, `:3435`, `:3570`).
  Bound the idle wait and complete the follow on expiry. Done when a paused list
  releases the follow slot and a later request succeeds on the same connection.
- [M · 0.17.1] Keep the remote playback mirror aligned across slow frame reads.
  **Medium, builder.** The watcher times out a frame read and starts another on
  the same stream. If it consumed part of the prior frame, the next read treats
  the remaining bytes as a length
  (`crates/resonate-sync/src/peer.rs:597`,
  `crates/resonate-sync/src/quic.rs:319`, `:346`). Keep the partial frame across
  watch checks or restart the stream when the read is cancelled. Done when a frame
  split across the polling deadline can reach the mirror without desynchronising it.
- [M · 0.17.1] Keep Android's saved and applied sync settings in the same order.
  **Medium, architect.** Settings are published before the background preference
  write. A failed write leaves the screen on the new value, and rapid saves can
  write different generations from the one the service applies
  (`android/app/src/main/java/org/resonate/android/LibraryFeatures.kt:704`, `:963`,
  `:2393`, `android/app/src/main/java/org/resonate/android/ProviderAccountStore.kt:122`).
  Serialize persistence and application and keep the last saved settings on failure.
  Done when two rapid changes reach disk and the service in order and a failed save
  leaves the persisted configuration visible. Canonicalising a pairing key
  during startup must also preserve a newer settings change.
- [M · 0.17.1] Save a newly minted phone pairing code before offering it to other devices.
  **Medium, architect.** Both apps can show the native service's new code after
  its first save fails. The next launch reads a blank key and creates another group
  (`crates/resonate-mobile/src/sync.rs:357`,
  `android/app/src/main/java/org/resonate/android/LibraryFeatures.kt:982`, `:991`,
  `ios/Sources/Resonate/Features.swift:887`, `:900`). Commit the code before
  presenting it for pairing. Done when the first key write fails and recovers
  with no shareable code lost on restart.
- [M · 0.17.1] Retry nearby changes until the other device acknowledges them.
  **Medium, architect.** A nearby-only look clears owed changes after scheduling
  a send. If that stream fails, the connection can stay up and the sender's
  per-device digest set suppresses the same change
  (`crates/resonate-sync/src/service.rs:2408`, `:2521`, `:3285`, `:3320`,
  `crates/resonate-sync/src/peer.rs:1057`). Record acknowledgement before suppressing
  retries or arrange a whole replay after a failed tell. Done when a transient
  write failure on a connected nearby peer clears and the change reaches it.
- [L · 0.17.1] Detect a rebuilt server space even after its cursor catches up.
  **Medium, architect.** A client notices a rebuilt space only when the reported
  head is below its cursor. If another device has already written at least that
  far, the old cursor skips the rebuilt space's earlier records and the client
  keeps its own prior changes marked sent
  (`crates/resonate-sync/src/service.rs:1657`, `:1671`,
  `crates/resonate-server/src/serve.rs:829`, `:1117`,
  `crates/resonate-server/src/store.rs:556`, `:868`). Give a space an incarnation
  that can be compared across reconnects with compatible wire and store changes.
  Done when a returning client catches up after another device recreates an
  expired space and writes past the returning client's cursor.
- [M · 0.17.1] Clear a synced favourite across copies of the same recording.
  **Medium, builder.** Desktop sync resolves an unfavourite to one row, while
  the next look still reads a favourite on another copy and can send it back
  (`crates/resonate-library/src/synced.rs:766`, `:1209`, `:1226`,
  `crates/resonate-sync/src/ledger.rs:1488`). Apply the unfavourite to confirmed
  copies of the recording. Done when the second copy carries the only favourite,
  a remote removal clears it, and a subsequent look keeps the removal.
- [M · 0.17.1] Retry a desktop sync start after its lock becomes available.
  **Medium, architect.** A startup blocked by another process or refused by
  a temporary error is recorded as the current settings. The watcher retries
  only after an arrangement change
  (`crates/resonate/src/sync.rs:251`, `:283`, `:328`, `:509`).
  Track successful starts separately and retry an enabled service with a bounded
  delay. Done when a second process begins syncing after the first releases the
  lock, and a transient startup failure recovers with the settings unchanged.
- [M · 0.17.1] Give desktop command-line fetch batches a terminal result.
  **Medium, builder.** The CLI waits for every requested recording in the display
  transfer list. The service can remove a row when the file has arrived via
  another operation or a fetch claim exists, leaving the terminal command waiting
  with no deadline (`crates/resonate/src/sync.rs:1450`, `:1465`,
  `crates/resonate-sync/src/service.rs:3601`). Use authoritative outcomes for
  individual requests and bound the batch wait. Done when an already-held song,
  a concurrent fetch and an unreachable device all complete the command with
  an accurate result.
- [M · 0.17.1] Keep device IDs with desktop handoff choices.
  **Medium, builder.** The desktop turns each handoff's device ID into its label,
  then resolves the first matching label when the listener continues. Devices
  with the same name can open the wrong queue
  (`crates/resonate/src/sync.rs:1431`, `:1585`, `:1866`, `:1882`).
  Pass the selected ID through the UI and surface ambiguity in name-based commands.
  Done when either of two same-named devices can be selected and continues its own queue.
- [S · 0.17.1] Resolve previously received song aliases when continuing a handoff on a phone.
  **Medium, builder.** Handoff playback uses the name index alone. A song that
  sync recognises by its received recording key can be skipped, losing its
  playback position or leaving the handoff empty
  (`crates/resonate-mobile/src/sync.rs:1045`, `:1662`, `:1932`). Reuse the visible
  received-key resolver for queue rows. Done when a received file with different
  tags resumes under its original recording identity at the handed-off position.
- [M · 0.17.1] Expire the live display when a remote playback mirror goes quiet.
  **Medium, architect.** The watch lease stays active as a view polls it, so
  the last mirror can keep showing a moving position and active controls after
  the device leaves
  (`crates/resonate-sync/src/remote.rs:385`,
  `crates/resonate-sync/src/service.rs:1070`, `:3482`,
  `crates/resonate-mobile/src/sync/driving.rs:354`,
  `ios/Sources/Resonate/DrivenDeviceView.swift:138`). Carry mirror expiry through
  the shared watch result and show that state in the controls. Done when
  traffic stops, the display expires and disables commands, and a fresh mirror
  restores the controls.
- [M · 0.17.1] Surface a failed phone sync restart and retry it.
  **Medium, architect.** The native restart stops the existing run before the
  replacement can fail. The iPhone then keeps the enabled preference with an
  off status that the settings page describes as starting
  (`crates/resonate-mobile/src/sync.rs:357`,
  `ios/Sources/Resonate/Features.swift:872`,
  `ios/Sources/ResonateLogic/Sync.swift:119`). Preserve the effective status,
  distinguish startup failure from an ongoing start, and make recovery
  reachable. Done when a failed replacement is visible and can recover
  without closing the app or changing a setting.
- [M · 0.17.1] Withdraw a handoff when its source clears the queue.
  **Medium, architect.** The desktop and phone catalogs return no resumption
  for an empty saved queue. The ledger leaves the previous handoff intact,
  so another device can still offer to continue the cleared queue
  (`crates/resonate-library/src/resumed.rs:59`,
  `crates/resonate/src/sync.rs:757`,
  `crates/resonate-mobile/src/sync.rs:1649`,
  `crates/resonate-sync/src/ledger.rs:1719`,
  `crates/resonate-sync/src/service.rs:966`). Send a newer empty handoff when
  a formerly published queue clears. Done when two ledgers retain a handed
  position and play order, withdraw the handoff after a clear, and ignore
  an older note that arrives afterwards.
- [M · 0.17.1] Keep unheld answers eligible across a full page of tied arrivals.
  **Medium, architect.** Phone documents can share one added second after
  bulk work or migration. When 500 rows fill a page at that second, the
  ledger advances to the next second and skips the rest
  (`crates/resonate-mobile/src/catalog.rs:57`, `:62`,
  `crates/resonate-mobile/src/sync/answers.rs:39`,
  `crates/resonate-sync/src/ledger/unheld.rs:95`, `:119`). Keep a stable
  secondary key in the cursor. Done when a page boundary inside one
  timestamp offers every newly held song its kept answers across restart.
- [M · 0.17.1] Check free space and freshness before replacing a phone's MusicBrainz copy.
  **Medium, architect.** Once a copy exists, the phone accepts a different
  export without checking whether it is older or whether the staged file fits
  alongside the kept copy
  (`crates/resonate-mobile/src/sync/copied.rs:280`, `:396`,
  `crates/resonate-sync/src/peer.rs:1343`,
  `crates/resonate-musicbrainz/src/mirror/offered.rs:90`, `:225`).
  Compare export versions and check the offered remaining bytes before
  downloading. Done when an older copy or a newer offer without room
  leaves the existing mirror intact and a newer offer with room replaces it.
- [M · 0.17.1] Coordinate removing the phone's MusicBrainz copy with its receiver.
  **Medium, architect.** Removal unlinks the receive paths without stopping
  the keeper. An open download can retain the file's space, and the next look
  can fetch the copy again
  (`crates/resonate-mobile/src/sync/copied.rs:104`, `:174`, `:344`,
  `crates/resonate-sync/src/peer.rs:1376`). Settle or cancel the receive before
  reclaiming the copy and make the post-removal setting explicit. Done when
  removal during a refresh reclaims the copy and a later look honours the
  decision to remove it.
- [M · 0.17.1] Carry renewed TLS credentials into a packaged sync service.
  **Medium, architect.** The unit supplies a credential snapshot at startup,
  and the server reads the chain and key once. Replacing the installed PEMs
  leaves the running listener on the old chain
  (`packaging/server/resonate-server.service:6`,
  `crates/resonate-server/src/main.rs:236`, `:264`,
  `crates/resonate-server/src/serve.rs:176`). Provide a supported renewal
  handoff that updates or restarts the TLS listeners after validating the
  new pair. Done when the next handshake after renewal presents the new
  chain and a failed renewal preserves the working credentials.
- [S · 0.17.1] Recheck held answers after a catalog addition during settlement.
  **Low, builder.** The ledger remembers the catalog revision after checking for new songs.
  A song committed after the final page read can have its revision consumed before its
  kept answers are checked (`crates/resonate-sync/src/ledger/unheld.rs:80`, `:91`, `:99`).
  Remember the starting revision. Done when an addition just after the last page receives
  its waiting answer on the next settle without an unrelated catalog edit.
- [S · 0.17.1] Prefer usable LAN addresses before capping advertisements.
  **Low, builder.** Numeric order can keep several bridge addresses while dropping
  the actual LAN address from a beacon or WebSocket punch
  (`crates/resonate-sync/src/nearby.rs:185`, `:202`,
  `crates/resonate-sync/src/service.rs:1318`, `:2887`). Use the system's route and
  interface information to rank addresses before the cap. Done when a LAN address
  remains advertised alongside several virtual interfaces.
- [M · 0.17.1] Refresh nearby multicast memberships when interfaces change.
  **Low, builder.** The IPv6 beacon listener joins only the interfaces present when
  sync starts, so a device can miss multicast discovery after Wi-Fi comes up if
  Bonjour is unavailable (`crates/resonate-sync/src/nearby.rs:99`, `:116`,
  `crates/resonate-sync/src/service.rs:770`). Update memberships as usable interfaces
  change. Done when an interface appearing after launch can receive beacons without
  restarting sync.
- [S · 0.17.1] Validate the desktop sync command's source option.
  **Low, builder.** The command accepts `--from` on its own and alongside
  `--handoff`, then silently disregards it
  (`crates/resonate-cli/src/grammar.rs:351`, `:360`,
  `crates/resonate/src/connected.rs:23`). Tie it to `--fetch` in the grammar.
  Done when both unused forms are refused and the existing fetch and handoff
  commands still parse.
- [S · 0.17.1] Retry Android MediaStore publication after adopting a received song.
  **Low, builder.** A song is already indexed when the update that clears its
  MediaStore pending state can fail. The error instead increments the receive
  refusal counter for a song that has already left the receive queue
  (`android/app/src/main/java/org/resonate/android/LibraryFeatures.kt:1184`, `:1193`,
  `android/app/src/main/java/org/resonate/android/MusicDocuments.kt:177`).
  Track publication separately and retry it while the app stays open.
  Done when a rejected publication later succeeds without copying or indexing
  the song twice.
- [S · 0.17.1] Keep the playhead in a handoff that fits the server's record budget.
  **Low, builder.** The handoff is bounded by rows, but a long set of titles
  and credits can exceed the sealed record limit. Then the service clears
  the note as sent without appending it
  (`crates/resonate-sync/src/catalog.rs:56`, `crates/resonate-sync/src/ledger.rs:4308`,
  `crates/resonate-sync/src/service.rs:2455`, `:2466`). Fit the queue window to the
  record budget while retaining the playing row and its offset. Done when an
  oversized queue can be continued from another network at the expected position.
- [S · 0.17.1] Let desktop listeners stop driving a device after it goes offline.
  **Low, builder.** The drive panel remains selected after its device disappears
  or is removed, and the stop action exists only on an online device's row
  (`crates/resonate-ui/src/views/settings/sync.rs:565`, `:1323`, `:1395`).
  Keep a stop action available for the selected device and clear a forgotten
  device's panel. Done when an offline device can be dismissed and removal
  immediately leaves the drive panel.
- [M · 0.17.1] Reconcile app-private received files after an interrupted landing.
  **Low, architect.** A receive can rename bytes into the holding folder before
  an error leaves them untracked. Cleanup can also delete the catalog row
  before unlinking the file
  (`crates/resonate-mobile/src/sync.rs:982`, `:1036`, `:1244`, `:1798`,
  `crates/resonate-mobile/src/catalog.rs:131`). Keep the file owned through the
  catalog commit and make cleanup retryable. Done when a failed alias write,
  a concurrent adoption and a failed unlink leave each file tracked or reclaimed.
- [M · 0.17.1] Apply the phone's metered-network setting to existing drive streams.
  **Low, builder.** Switching to metered data with transfers disabled blocks
  new peer connections. A previously opened connection can keep sending
  mirror frames and accepting drive commands
  (`crates/resonate-sync/src/service.rs:1942`, `:3744`,
  `crates/resonate-sync/src/peer.rs:228`, `:427`, `:457`). Enforce the network
  policy on existing peer requests and streams. Done when an established
  watch and subsequent commands stop on a metered transition and work again
  after the connection is permitted.
- [S · 0.17.1] Clear stale Android sync landing notices when the queue settles.
  **Low, builder.** The poll leaves the previous landing error on screen when
  the waiting queue becomes empty after the final refusal
  (`android/app/src/main/java/org/resonate/android/LibraryFeatures.kt:1064`, `:1129`,
  `:1192`, `:1211`). Derive the notice from pending work and report a localised
  message for a landing failure. Done when the final refusal clears the pending
  notice and a later received song shows its own status.
- [S · 0.17.1] Show a definite Android remote-watch failure when sync stops.
  **Low, builder.** The drive panel drops read failures and displays its
  waiting state even after a device is removed or sync turns off
  (`android/app/src/main/java/org/resonate/android/ui/settings/DrivenDevice.kt:154`,
  `crates/resonate-mobile/src/sync/driving.rs:377`). Preserve the typed outcome
  and retire the watch for a known unavailable device. Done when those
  transitions replace the waiting state and a transient read can still recover.
- [S · 0.17.1] Show the actual chosen iOS landing folder when it is unavailable.
  **Low, builder.** The folder picker filters out an unreadable saved
  destination and shows another folder selected, while the landing logic
  holds the songs until the saved one returns
  (`ios/Sources/Resonate/SyncView.swift:485`, `:521`,
  `ios/Sources/ResonateLogic/Sync.swift:211`). Show the unavailable choice and
  let the listener choose a reachable replacement. Done when the picker and
  the landing status agree with one available and one unavailable folder.
- [S · 0.17.1] Hide the old iOS pairing code while a new group is starting.
  **Low, builder.** A confirmed rotation changes the configuration while the
  sheet can still copy and share the previous code from the live summary
  (`ios/Sources/Resonate/Features.swift:671`, `:887`,
  `ios/Sources/Resonate/SyncView.swift:434`, `:442`). Withhold the old code at
  confirmation and reset visibility when the replacement is ready.
  Done when a delayed restart leaves the old code unavailable in the sheet.
- [S · 0.17.1] Restore the saved iOS sync route after a settings write fails.
  **Low, builder.** The connection picker keeps its local choice after the
  main configuration reverts to the saved route
  (`ios/Sources/Resonate/SyncView.swift:565`, `:576`, `:680`,
  `ios/Sources/Resonate/Features.swift:592`). Synchronize the displayed choice
  with the persistence result. Done when a failed route change restores the
  prior selection and the control reflects the active route.
- [S · 0.17.1] Advance the phone's amendment timer only after an amendment check.
  **Low, builder.** A network wake inside the hourly interval can see an
  unchanged copy, skip amendments and reset the timer anyway. Frequent wakes
  postpone amendments indefinitely
  (`crates/resonate-mobile/src/sync.rs:504`,
  `crates/resonate-mobile/src/sync/copied.rs:241`, `:308`). Record the last
  actual amendment request. Done when repeated network wakes within an hour
  still allow the next due tick to request amendments.
- [M · 0.17.1] Commit each imported phone sync certificate from its own stage.
  **Low, builder.** Two imports can write the same staging file. The rename
  can report success before the new certificate and its directory are synced
  (`crates/resonate-mobile/src/sync/certificate.rs:8`, `:23`).
  Give each import an owned stage and persist the validated bytes and the
  replacement before reporting success. Done when racing imports and a
  failed stage leave one complete validated certificate installed.
## Catalog
- [S · 0.17.1] Settle a pending removal before a scan prunes its file. **Low, builder.**
  A delete whose run ended after the trash took the file is settled when the catalog next opens or
  before the next delete (`deleted::settle`, `removals_pending`), but an open that could not take
  the walk lock skips it, and a scan then prunes the gone row first, so the every-device notice for
  that song is never queued (`crates/resonate-library/src/deleted.rs`, `scan.rs`). Settle pending
  removals at the start of a scan, under its walk guard. Done when a run ended after the move,
  an open behind another walker and a scan leave the deletion noticed to devices exactly once.
- [M · 0.17.1] Keep a song moved into a root that a scan names alone. **Medium, architect.**
  `resonate scan R2` walks only the destination root, and its tidy of the other registered roots
  forgets the old row as gone before any later scan can pair it with the arrival, so the song keeps
  its file but loses its ID, favourite, listens and queue place (`crates/resonate-library/src/scan.rs`,
  `tidy_the_roots_beside`; the size-only guard there cannot tell an unsettled arrival from a
  same-sized stranger). Let the tidy hold a gone row whose likeness (size, length, codec, tagged
  names) an unsettled arrival under a walked root shares, or pair it then. Done when a song moved
  from R1 to R2 and scanned with `scan R2` keeps its ID and favourite, and the existing tidy tests
  with same-sized fixtures still forget what is truly gone.
- [M · 0.17.1] Keep organise's filename swaps together when a move fails or is cancelled.
  **Medium, architect.** The park and landing are ordinary moves, so cancellation or an I/O refusal
  can commit only the park and leave the catalog pointing at a temporary filename
  (`crates/resonate-library/src/organise.rs:2104`, `crates/resonate-library/src/organise.rs:2300`,
  `crates/resonate-library/src/organise.rs:2384`). Apply the entire cycle as one unit and roll back
  an incomplete one. Done when a cancelled swap, an injected landing refusal and a swap at a
  batch boundary leave both file paths and catalog entries consistent.
- [M · 0.17.1] Keep destination files when a move lacks an atomic no-replace operation.
  **Low, architect.** The fallback can use a replacing rename after a separate existence
  check, so a concurrently created destination file can be lost
  (`crates/resonate-library/src/unreplacing.rs:23`). Use a platform operation that
  reserves the destination atomically or refuse the move on that volume. Done when a
  fallback test preserves both the source and a competing destination.
- [M · blocked: platform] On macOS and Windows a retag that grows a hard-linked track's head while it
  plays still shifts the bytes under the decoder: neither can tell who holds a file
  (`Shifting::readers_can_be_asked` is false), so the copy is written back over the same inode as
  before rather than refusing every such write; Linux and Android wait for the readers under a lease

## Mobile catalog
- [M · 0.17.1] Export a phone's listening history from one snapshot. **Low, architect.**
  The desktop's export now holds one read transaction across its pages (`KeptListens::history_pages`,
  `PinnedHistory`), but the phones' catalog answers the unpinned default
  (`crates/resonate-mobile/src/history.rs`): its one connection sits behind a mutex that a pinned
  export would hold across pages, so a listen counted or forgotten between pages can be missed or
  written twice. Read the phone's export from one snapshot (a second read-only connection, or the
  pages copied under one hold) without holding up the player. Done when a listen counted and one
  forgotten between pages leave the export with every listen present at its start exactly once.
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
- [M · 0.17.1] Keep a phone document's analysis when reopening an unchanged file.
  **Low, builder.** Android's open-with path can reindex a known document without a
  size or modification stamp. That clears the old stamp and triggers a reset of its
  cached analysis (`android/app/src/main/java/org/resonate/android/OpenedFiles.kt:186`,
  `crates/resonate-mobile/src/catalog_extras.rs:190`,
  `crates/resonate-mobile/src/catalog.rs:138`). Preserve known stamps when a read provides
  no newer one, while still invalidating analysis on confirmed changes. Done when indexing
  a stamped file from open-with keeps the analysis and scanning a changed file refreshes it.
## Providers and network
- [M · 0.17.1] Ask for uncoded audio from the first seekable HTTP request.
  **Medium, architect.** The first Jellyfin, native TIDAL FLAC and relayed hifi stream
  requests can negotiate gzip while later requests for the same reader ask for identity
  coding. The decoded first response then has no known length, which disables seeking, and can
  supply a validator that conflicts with the identity-coded range response
  (`crates/providers/jellyfin/src/lib.rs:270`, `:728`,
  `crates/providers/tidal/src/played.rs:202`,
  `crates/providers/tidal/src/hifi_served.rs:197`,
  `crates/resonate-fetch/src/ranged.rs:149`,
  `crates/resonate-providers/src/in_place.rs:275`). Use the same coding on the initial and
  reopened requests. Done when provider tests with a server that offers gzip see identity
  on both requests, and the reader knows the uncoded length and returns the expected bytes
  after a seek.
- [M · 0.17.1] Restart a MusicBrainz archive download after a stale partial fails.
  **Low, builder.** The dump checksum protects installation, but a resumed answer
  that fails verification after returning bytes ends the job. A whole answer
  shorter than the held prefix can fail before verification and leave that
  partial in place for the next attempt
  (`crates/resonate-musicbrainz/src/download.rs:289`, `:331`, `:360`,
  `crates/resonate-fetch/src/ranged.rs:201`). Retry once from zero when the
  saved prefix is incompatible. Done when both cases complete with a new
  verified archive from a valid whole response and a bad whole response still
  fails checksum verification.

## Formats
- [M · blocked: symphonia] A file embedding a huge picture still costs one materialisation:
  symphonia 0.6.1 reads it into its own buffer before `probe_cover_art` can weigh it; its
  `limit_visual_bytes` is honoured by no reader

## macOS

