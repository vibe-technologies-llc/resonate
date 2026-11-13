the-decade = { $decade ->
    [1920] the twenties
    [1930] the thirties
    [1940] the forties
    [1950] the fifties
    [1960] the sixties
    [1970] the seventies
    [1980] the eighties
    [1990] the nineties
   *[other] the { NUMBER($decade, useGrouping: "false") }s
}
decade-named = { $decade ->
    [1920] The twenties
    [1930] The thirties
    [1940] The forties
    [1950] The fifties
    [1960] The sixties
    [1970] The seventies
    [1980] The eighties
    [1990] The nineties
   *[other] The { NUMBER($decade, useGrouping: "false") }s
}
genre-of-a-decade = { $genre } of { $decade }
more-like = More like { $seed }
deep-cuts = { $artist }, deep cuts
discover = Discover { $artist }
back-to = Back to { $artist }
unheard-genre = Unheard { $genre }
on-repeat = On repeat
forgotten-favourites = Forgotten favourites
old-flames = Old flames
second-chances = Second chances
starred-never-played = Starred, never played
fresh-arrivals = Fresh arrivals
short-and-sharp = Short and sharp
surround = Surround
in-mono = In mono
slow-burners = Slow burners
never-heard = Never heard
most-played = Most played
recently-added = Recently added
back-catalogue = Back catalogue
hi-res = Hi-res
long-players = Long players
reason-decade = Everything from { $decade }
reason-genre-of-a-decade = One genre, from { $decade }
reason-genre = Everything filed under it
reason-artist = Everything they made
reason-never-heard = You have never heard these
reason-most-played = The ones you come back to
reason-recently-added = The newest in the library
reason-fresh-arrivals = New this month and not played yet
reason-back-catalogue = Not heard for over a year
reason-on-repeat = What you keep going back to this month
reason-kindred-artists = Artists sharing its genres, that you hear less
reason-deep-cuts = What you pass over by an artist you play
reason-artist-to-discover = An artist you hold and have never played
reason-unheard-genre = A genre you play, in songs you have not
reason-forgotten-favourites = Starred, then left alone for half a year
reason-old-flames = Played often once, quiet for half a year
reason-second-chances = Played once, over a month ago
reason-starred-unheard = You marked these before hearing them
reason-artist-left-behind = You played them often, then stopped
reason-hi-res = Better than a CD
reason-long-players = Ten minutes and over
reason-slow-burners = Six to ten minutes
reason-short-and-sharp = Two minutes or less
reason-surround = More than two channels
reason-mono = One channel
kind-for-you = Made for you
kind-rediscover = Rediscover
kind-listening = From your listening
kind-eras = Eras
kind-genres = Genres
kind-artists = Artists
kind-sound = Sound
