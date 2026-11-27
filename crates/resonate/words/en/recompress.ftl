format-of = { $codec } { $rate } kHz, { $bits } bit
candidates = { $count ->
    [one] One FLAC file holds { $bytes }.
   *[other] { $count } FLAC files hold { $bytes }.
} Nothing is written until --apply says so.
shrunk = { $file }: { $before } to { $after }
kept = { $file }: kept as it was, { $why }
failed = { $file }: kept as it was, the copy could not be made: { $error }
saved = { $count ->
    [one] One file written smaller
   *[other] { $count } files written smaller
}, { $saved } saved.
not-held = { $file } is not a file the library holds.
nothing-held = The library holds no whole FLAC file to re-compress.
nothing-stored = The library holds no file.
why-no-smaller = since it is no smaller written again
why-unverified = since the copy did not read back as it went in
why-changed-meanwhile = since the file changed while it was written again
why-halted = since it was stopped before it was done
why-not-a-flac = since it is not a FLAC file
why-vaulted = since its song is kept in the vault
why-cut-by-sheet = since a cue sheet cuts it into songs
why-outside-the-library = since it is under no folder of the library
why-head-unread = since its FLAC header could not be read
why-id3-after-the-audio = since it carries an ID3v1 tag after the sound
why-ape-after-the-audio = since it carries an APE tag after the sound
why-beyond-the-encoder = since its { $rate } kHz, { $bits } bit sound is beyond what the encoder writes, unless --to names a ceiling
why-tied-to-the-samples = since its { $block } block names places in the sound that a new rate or depth would move
why-disagrees-with-its-digest = since its sound does not match the checksum it carries
why-wider-than-its-depth = since its samples are wider than the depth it declares
why-turns-partway = since its sound changes shape partway through
why-undecodable = since its sound could not be decoded
why-silent = since it holds no sound
why-held-under-other-names = since it is linked under other names, which would keep the old copy
why-attributes-not-carried = since the copy could not be given the file's owner, permissions or attributes
