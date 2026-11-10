back-hint = Back to where this was opened from
play-hint = Play everything here, in order
next-hint = Play this next
queue-hint = Add this to the end of the queue
open-hint = See what the server holds under this
play-this-hint = Play this, and what follows it here
not-played-here-hint = This service's songs are not played from here
keep-hint = Keep this song in the music folder
keep-all-hint = Keep every song here in the music folder, as a playlist
ask-again-hint = Ask the server again
search-placeholder = Search the server, then press enter
nothing-here = Nothing here
nothing-listed = Search to begin
favourites = Favourites
search-first = This service lists nothing until it is asked. Search above for an artist, an album or a song.
choose-hint = Browse this service
choose-among-hint = Choose which service to browse
nothing-found = Nothing found
asking = Asking the server…
asking-more = What it holds here appears as soon as it answers.
nothing-found-more = The service has nothing for that search. Try other words.
nothing-here-more = The service holds nothing under this.
try-again = Try again
play = Play
keep = Keep
favourite-unchanged = Couldn't change that favourite — { $failure }
unread = Couldn't read that — { $failure }
artists = { $count ->
    [one] { $count } artist
   *[other] { $count } artists
}
albums = { $count ->
    [one] { $count } album
   *[other] { $count } albums
}
mixes = { $count ->
    [one] { $count } mix
   *[other] { $count } mixes
}
songs = { $count ->
    [one] { $count } song
   *[other] { $count } songs
}
queued-and-playing = { $count ->
    [one] { $count } song queued and playing
   *[other] { $count } songs queued and playing
}
to-play-next = { $count ->
    [one] { $count } song to play next
   *[other] { $count } songs to play next
}
added-to-the-queue = { $count ->
    [one] { $count } song added to the queue
   *[other] { $count } songs added to the queue
}
