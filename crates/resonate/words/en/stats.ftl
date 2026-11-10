tracks-most = TRACKS MOST LISTENED TO
albums-most = ALBUMS MOST LISTENED TO
artists-most = ARTISTS MOST LISTENED TO
track = TRACK
album = ALBUM
artist = ARTIST
plays-heading = PLAYS
listened-heading = LISTENED
nothing-played = nothing was played
nothing-played-across = nothing was played { $across }
summary = { $plays ->
    [one] { $plays } play
   *[other] { $plays } plays
} { $across } · { $listened } listened · { $tracks ->
    [one] { $tracks } track
   *[other] { $tracks } tracks
} · { $albums ->
    [one] { $albums } album
   *[other] { $albums } albums
} · { $artists ->
    [one] { $artists } artist
   *[other] { $artists } artists
}
all-told = all told
over-the-last-week = over the last week
over-the-last-month = over the last month
over-the-last-year = over the last year
seconds = { $seconds }s
minutes = { $minutes }m
hours-and-minutes = { $hours }h { $minutes }m
