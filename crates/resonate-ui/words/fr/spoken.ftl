artist = { $name }, { $albums ->
        [one] { $albums } album
       *[other] { $albums } albums
    }, { $tracks ->
        [one] { $tracks } piste
       *[other] { $tracks } voies
    }
