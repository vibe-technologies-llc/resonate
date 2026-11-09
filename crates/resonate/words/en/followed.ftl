started-from = { $name }: started from { $service }, { $kept }
read-from = { $name }: read from { $service }, { $kept }
followed-on = { $name }: followed on { $service }
moved-once-on = { $name }: moved to Resonate once, and no longer followed on { $service }
let-go = { $playlists ->
    [one] { $playlists } playlist let go
   *[other] { $playlists } playlists let go
}
deleted-there = { $playlists ->
    [one] { $playlists } playlist deleted there
   *[other] { $playlists } playlists deleted there
}
unread = { $playlists ->
    [one] { $playlists } playlist unread
   *[other] { $playlists } playlists unread
}
none-in-step = no playlist is kept in step with a link; `resonate import <LINK> --upkeep in-step` keeps one
spotify-not-signed-in = the Spotify account is followed but not signed in; `resonate spotify` signs in
its-link = its link
was-already = { $name } was already { $kept }
in-step-now = { $name } is kept in step with { $link } now
left-as-it-stands = { $name } is left as it stands, and { $link } is not read again on its own
no-longer-follows = { $name } no longer follows a link; its rows stay
came-from-no-link = { $name } came from no link
came-from = { $service }, { $upkeep }
once = once
in-step = in step
names-no-playlist = { $name }: { $service } names no playlist now; its rows stay as they were
read-again = { $name }: read again from { $service }, { $moved }
imported-once = imported once
kept-in-step = kept in step
nothing-to-add = nothing to add
added = added { $songs ->
    [one] { $songs } song
   *[other] { $songs } songs
}
took-out = took out { $songs ->
    [one] { $songs } song
   *[other] { $songs } songs
}
added-and-took-out = added { $added ->
    [one] { $added } song
   *[other] { $added } songs
} and took out { $removed ->
    [one] { $removed } song
   *[other] { $removed } songs
}
wanted = wanted { $songs ->
    [one] { $songs } song
   *[other] { $songs } songs
} the library lacks, which `resonate poll` fetches
not-wanted = { $songs ->
    [one] { $songs } song
   *[other] { $songs } songs
} could not be wanted
unnamed = { $songs ->
    [one] { $songs } song
   *[other] { $songs } songs
} named nothing MusicBrainz knows
