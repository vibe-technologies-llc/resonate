what-the-rankings-count = Rankings count the songs still in your library.
nothing-played = Nothing was played in { NUMBER($year, useGrouping: "false") }
play-something = Play something and this fills itself in.
song-of-the-year = Your song of the year
artist-of-the-year = Your artist of the year
album-of-the-year = Your album of the year
genre-of-the-year = Your genre of the year
my-year = My { NUMBER($year, useGrouping: "false") } on Resonate
of-music = { $listened } of music
top-song = Top song: { $song }
top-artist = Top artist: { $artist }
top-album = Top album: { $album }
top-genre = Top genre: { $genre }
longest-streak = { $count ->
    [one] Longest streak: { $count } day
   *[other] Longest streak: { $count } days
}
lossless = { $share } lossless
across-the-days = { $across }, over { $days }
your-year = Your year
biggest-month = { $month } was your biggest month
your-day = Your day
listened-most-at = You listened most at { $hour }:00
biggest-day-was = Your biggest day was { $date }: { $listened }
your-streak = Your streak
in-a-row = { $days } in a row
streak-from = From { $date }. { $biggest }
your-biggest-day = Your biggest day
joined-your-library = { $songs } joined your library.
new-to-you = New to you
heard-for-the-first-time = { $songs } heard for the first time
new-to-your-library = New to your library
your-sound = Your sound
above-cd-quality = { $share } above CD quality. Counted over the songs in your library.
where-it-began = Where it began
by-on = { $by }, on { $on }
only-on = On { $on }
that-was-your-year = That was your { NUMBER($year, useGrouping: "false") }
named-by = { $name } by { $by }
songs = { $count ->
    [one] { $count } song
   *[other] { $count } songs
}
artists = { $count ->
    [one] { $count } artist
   *[other] { $count } artists
}
albums = { $count ->
    [one] { $count } album
   *[other] { $count } albums
}
days = { $count ->
    [one] { $count } day
   *[other] { $count } days
}
minutes = { $count ->
    [one] { $count } minute
   *[other] { $count } minutes
}
hours = { $count ->
    [one] { $count } hour
   *[other] { $count } hours
}
small-hours = The small hours are yours.
start-the-day = You start the day with music.
mornings = Your mornings have a soundtrack.
afternoons = Your afternoons have a soundtrack.
evenings = Your evenings have a soundtrack.
late = You listen late.
