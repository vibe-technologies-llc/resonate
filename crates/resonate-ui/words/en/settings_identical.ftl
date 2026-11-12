identical-songs = Identical songs
page-about = Files holding the same bytes, folded into the song they copy
hint = Finds files holding exactly the same bytes, such as Song (1).flac beside Song.flac or a song copied into another folder under a new name. Of each set the file not numbered like a copy, and among those the one catalogued first, is the song; the others are its copies. Fold moves a copy's plays, favourite, playlist entries and queue rows to that song and sends the copy to the trash. A numbered file whose bytes differ from every other is left where it is. Nothing is folded on its own: Find lists what would be, and only Fold changes anything.
also-called = duplicates duplicate copies numbered same identical dedupe fold merge twin
find = Find
finding = Finding…
fold = Fold
folding = Folding…
press-again-to-fold = Press again to fold
find-first = Find first, so the songs held twice are on screen before anything is folded.
moved-under-the-search = The catalog moved since the last search, so it was taken down. Find again to see what would be folded now.
ask-again-to-fold = Press it again to fold the copies. Each copy's file goes to the trash and its plays, favourite, playlist entries and queue rows go to the song it copies.
none-held-twice = No two files hold the same bytes.
would-fold = { $copies ->
    [one] would fold { $copies } copy
   *[other] would fold { $copies } copies
} into { $songs ->
    [one] { $songs } song
   *[other] { $songs } songs
} · { $differing ->
    [one] { $differing } numbered file differs and stays
   *[other] { $differing } numbered files differ and stay
} · nothing has been folded
copies-of-it = { $count ->
    [one] Copy: { $names }
   *[other] { $count } copies: { $names }
}
and-more = and { $rest } more
note = Only files holding exactly the same bytes are copies of one another. Find lists them; only Fold sends a copy to the trash, and the song keeps everything the copy had.
