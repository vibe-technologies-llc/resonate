queued = { $title } is queued
already-held = { $title } is already held, so nothing was queued
list-kept = { $name }: { $queued ->
    [one] { $queued } song
   *[other] { $queued } songs
} queued, { $held ->
    [one] { $held } song
   *[other] { $held } songs
} already held
left-out = { $held }, { $untitled ->
    [one] { $untitled } song
   *[other] { $untitled } songs
} left out with no title
walked = landed { $landed } | gone { $gone } | unkept { $unkept }
premium = Premium formats available
without-premium = signed in, but this account has no Premium formats
not-signed-in = not signed in
tool-missing = yt-dlp could not be run; it is a separate install
check-song-gone = the song this check asks about has left YouTube, so nothing could be told
could-not-check = YouTube could not be asked just now; try again in a while
