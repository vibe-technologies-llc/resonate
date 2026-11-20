of-music = de musique
across = { $songs ->
    [one] { $songs } chanson
   *[other] { $songs } chansons
} · { $artists ->
    [one] { $artists } artiste
   *[other] { $artists } artistes
} · { $albums ->
    [one] { $albums } album
   *[other] { $albums } albums
}
formats = { $lossless } sans perte · { $finer } au-dessus de la qualité du disque compact
top-song = Meilleure chanson
top-artist = Haut artiste
top-album = Meilleur album
top-genre = Le meilleur genre
