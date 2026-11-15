offering = Offering the catalog on the network from port { $port } as { $udn }
holds = It holds { $songs ->
    [one] { $songs } song
   *[other] { $songs } songs
}, { $albums ->
    [one] { $albums } album
   *[other] { $albums } albums
}, { $artists ->
    [one] { $artists } artist
   *[other] { $artists } artists
} and { $playlists ->
    [one] { $playlists } playlist
   *[other] { $playlists } playlists
}
stopped = Stopped offering the catalog
