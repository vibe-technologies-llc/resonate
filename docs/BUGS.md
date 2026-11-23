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
- [M · blocked: platform] On macOS and Windows a retag that grows a hard-linked track's head while it
  plays still shifts the bytes under the decoder: neither can tell who holds a file
  (`Shifting::readers_can_be_asked` is false), so the copy is written back over the same inode as
  before rather than refusing every such write; Linux and Android wait for the readers under a lease

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

