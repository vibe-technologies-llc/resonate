nothing-favourite = No favourites yet
nothing-matches = No favourites match
nothing-favoured-matches = Nothing you favoured matches this search.
how-to-favour = Press the star on a track, an album or an artist to keep it here.
artists = { $count ->
    [one] { $count } artist
   *[other] { $count } artists
}
albums = { $count ->
    [one] { $count } album
   *[other] { $count } albums
}
favourites = Favourites
