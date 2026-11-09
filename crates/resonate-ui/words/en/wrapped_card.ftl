of-music = of music
across = { $songs ->
    [one] { $songs } song
   *[other] { $songs } songs
} · { $artists ->
    [one] { $artists } artist
   *[other] { $artists } artists
} · { $albums ->
    [one] { $albums } album
   *[other] { $albums } albums
}
formats = { $lossless } lossless · { $finer } above CD quality
top-song = Top song
top-artist = Top artist
top-album = Top album
top-genre = Top genre
