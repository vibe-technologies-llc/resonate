artist = { $name }, { $albums ->
        [one] { $albums } album
       *[other] { $albums } albums
    }, { $tracks ->
        [one] { $tracks } track
       *[other] { $tracks } tracks
    }
