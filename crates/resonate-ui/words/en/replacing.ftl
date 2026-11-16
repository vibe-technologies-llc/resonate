replace-lossy-songs = Replace lossy songs…
heading = Replace lossy songs · { $playlist }
heading-unnamed = Replace lossy songs
tick-all = Tick all
tick-all-hint = Tick every song that can be replaced
held = you hold it in { $codec }
wanted = wanted through MusicBrainz
not-matched = not matched on MusicBrainz
pending = already being replaced
reading = Reading the playlist…
nothing-lossy = Every song here is lossless
trash-note = Each lossy copy goes to the trash once its lossless copy is in your library.
keep = Cancel
keep-hint = Keep the lossy songs — { $keys }
replace = { $count ->
    [one] Replace 1 song
   *[other] Replace { $count } songs
}
replace-hint = Replace the ticked songs with lossless copies
folded = { $count ->
    [one] 1 song replaced
   *[other] { $count } songs replaced
}
wanted-count = { $count ->
    [one] 1 song wanted
   *[other] { $count } songs wanted
}
unmatched-count = { $count ->
    [one] 1 song not matched
   *[other] { $count } songs not matched
}
trash-refused = { $count ->
    [one] 1 lossy file couldn't be removed
   *[other] { $count } lossy files couldn't be removed
}
