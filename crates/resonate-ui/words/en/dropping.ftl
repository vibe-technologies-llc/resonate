no-folder-title = Choose a music folder first
no-folder-says = Songs dropped here are copied into your primary music folder. Set one in Settings under Library.
folder-gone-title = The music folder isn't there
folder-gone-says = If it is on a drive, mount it, then drop again.
nothing-title = Nothing here the library reads
nothing-says = Audio files, cue sheets and folders holding them are copied, with the covers and lyrics beside them; anything else is left where it is.
busy-title = Still copying
busy-says = Drop again once the songs being copied have landed.
ready-says = Copied as they are into
ready-to-file-says = Copied and filed by your layout into
weighing-title = Looking at what is dragged…
weighing-says = Drop when it says what would be copied.
stopped-after-copying = { $count ->
    [one] Stopped after copying { $count } file
   *[other] Stopped after copying { $count } files
}
copied-into = { $count ->
    [one] Copied { $count } file
   *[other] Copied { $count } files
} ({ $bytes }) into { $folder }
copied-into-beside-some-there = { $count ->
    [one] Copied { $count } file
   *[other] Copied { $count } files
} ({ $bytes }) into { $folder } · { $held } already there
already-in = Already in { $folder }, so nothing was copied
nothing-copied-source-gone = Nothing was copied — a file is not there any more
nothing-copied-unreadable = Nothing was copied — a file could not be read
nothing-copied-not-audio = Nothing was copied — a file is not a format the library reads
nothing-copied-nothing-inside = Nothing was copied — a file holds no audio
nothing-copied-already-there = Nothing was copied — a file is already in the folder
nothing-copied-already-held = Nothing was copied — a file is already in the folder, byte for byte
nothing-copied-unwritable = Nothing was copied — a file could not be written to the folder
nothing-copied-unverified = Nothing was copied — a file did not read back the same as it was sent
looks-audio = copied
looks-sheet = cue sheet, copied
looks-companion = beside a song, copied with it
looks-folder = folder, its audio, covers and lyrics are copied
looks-other = not audio, left out
looks-gone = not there
copy-the-songs = copy the songs
finish-copying-the-songs = finish copying the songs
drop-to-add = { $count ->
    [one] Drop to add { $count } item
   *[other] Drop to add { $count } items
}
ready-and-left-out = { $ready } · { $count ->
    [one] { $count } item
   *[other] { $count } items
} left out
and-more = and { $count } more
stopping = Stopping…
copying = Copying { $reached } of { $found } · { $copied } of { $whole }
stop-copying = Stop copying
