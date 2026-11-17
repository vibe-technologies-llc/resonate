written = { $count ->
    [one] Saved the lyrics of one song beside its file
   *[other] Saved the lyrics of { $count } songs beside their files
}
already-beside = { $count ->
    [one] One song already had these lyrics beside it
   *[other] { $count } songs already had these lyrics beside them
}
left-alone = { $count ->
    [one] A lyric sheet already stood beside one song, so it was left alone: choose Over a sheet already there to replace it
   *[other] Lyric sheets already stood beside { $count } songs, so they were left alone: choose Over a sheet already there to replace them
}
outranked = { $count ->
    [one] A richer lyric sheet beside one song is read ahead of these lyrics, so nothing was written for it
   *[other] Richer lyric sheets beside { $count } songs are read ahead of these lyrics, so nothing was written for them
}
none-kept = { $count ->
    [one] No fetched lyrics are kept for one song
   *[other] No fetched lyrics are kept for { $count } songs
}
no-file-of-its-own = { $count ->
    [one] One song is cut from a shared file or kept in the vault, so it has no lyric sheet of its own
   *[other] { $count } songs are cut from a shared file or kept in the vault, so they have no lyric sheet of their own
}
not-written = { $count ->
    [one] The lyrics of one song could not be saved beside it
   *[other] The lyrics of { $count } songs could not be saved beside them
}
