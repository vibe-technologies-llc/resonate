read-a-cover = read a cover
read-a-portrait = read a portrait
put-it-back = Put it back
nothing-to-share = That track isn't in the library, so there's no link to share
no-link-to-share = There's no link for that track
no-link-to-share-any = There's no link for any of those tracks
forgot-the-match = Forgot that release — the next lookup won't take it again
took-the-pressing = Took that pressing for this album
placed-on = Placed the track on that release
took-the-recording = The song is that recording now
not-taken = MusicBrainz no longer answers for that recording
gave-the-isrc = Gave the track that ISRC, looking it up by the code
already-on-a-release = That track is already on a release; Place on a release… moves it to another
the-rest-held-nothing = The rest of the discography held nothing worth keeping
read-more-of-the-discography = { $count ->
    [one] Read { $count } release more of the discography
   *[other] Read { $count } releases more of the discography
}
no-music-folder = Choose a music folder to keep songs in
no-title-to-keep = This song has no title to keep it under
already-held = Already in your library
already-walking = Another library task is still running — try again once it finishes
no-provider = No provider is set up to download it
change-the-playlist = change the playlist
want-that-track = want that track
stop-wanting-that-track = stop wanting that track
dismiss-that = dismiss that
bring-back-what-was-dismissed = bring back what was dismissed
mark-it-a-favourite = mark it a favourite
take-it-out-of-the-favourites = take it out of the favourites
hide-that-from-the-library = hide that from the library
show-that-in-the-library-again = show that in the library again
forget-that-delivery = forget that delivery
forget-that-match = forget that match
take-that-pressing = take that pressing
place-the-track-on-that-release = place the track on that release
treat-the-song-as-that-recording = treat the song as that recording
give-the-track-that-isrc = give the track that ISRC
look-the-track-up = look the track up
read-the-rest-of-the-discography = read the rest of the discography
keep-that-version = keep that version
forget-the-version-chosen = forget the version chosen
put-that-back = put that back
do-that-again = do that again
the-cookies-lapsed = The cookies lapsed at “{ $song }”. Sign in to YouTube again, and the rest stays queued
premium-lost = This account has lost Premium, so “{ $song }” and the rest stay queued until it is back
yt-dlp-missing = yt-dlp is missing. Install it, and “{ $song }” and the rest stay queued
the-service-stopped-answering = The service stopped answering at “{ $song }”, so the rest stays queued
kept-songs = { $count ->
    [one] Kept { $count } song
   *[other] Kept { $count } songs
}
songs-not-kept = { $count ->
    [one] { $count } song not kept
   *[other] { $count } songs not kept
}
kept-some-songs-not-others = Kept { $landed ->
    [one] { $landed } song
   *[other] { $landed } songs
}, { $missed ->
    [one] { $missed } song
   *[other] { $missed } songs
} not
keep-the-song = keep the song
list-kept = { $name }: { $queued } queued, { $held } already held
list-kept-leaving-out = { $name }: { $queued } queued, { $held } already held, { $unnamed } left out with no title
turn-online-on-to-read-a-playlist = Turn Online on to read a playlist from its link
the-link-names-no-playlist-any-more = The link names no playlist any more
read-the-playlist-from-its-link = read the playlist from its link
add-the-album = add the album
open-the-album = open the album
open-the-artist = open the artist
download-the-missing-tracks = download the missing tracks
preview-that-suggestion = preview that suggestion
read-that-album-s-pressings = read that album's pressings
turn-online-on-to-upload-a-file = Turn Online on to upload a file
uploading-file-to-catbox = Uploading file to Catbox…
public-catbox-file-link-is-on-the = Public Catbox file link is on the clipboard
only-whole-local-files-can-be-uploaded = Only whole local files can be uploaded with this build
upload-the-file-to-catbox = upload the file to Catbox
nothing-shown-was-left-to-take-out = Nothing shown was left to take out
already-in-that-order = Already in that order
back-in-hand-a-row-added-now = Back in hand: a row added now lands at the end
kept-in-that-order-and-it-was = Kept in that order, and it was already in it
every-row-names-a-file-that-is = Every row names a file that is there and appears once
count-that-play = count that play
nothing-left-to-put-back = Nothing left to put back
nothing-left-to-do-again = Nothing left to do again
read-the-library = read the library
keep-the-lyrics-timing = keep the lyrics' timing
send-the-earlier-plays = send the earlier plays
look-for-the-missing-covers-again = look for the missing covers again
start-the-scan = start the scan
finish-the-scan = finish the scan
start-the-lookup = start the lookup
finish-the-lookup = finish the lookup
keep-the-tracks-in-the-vault = keep the tracks in the vault
ask-the-providers = ask the providers
the-songs-could-not-be-kept = The songs could not be kept
write-the-tags-into-the-files = write the tags into the files
put-the-files-in-order = put the files in order
drop-that-library-folder = drop that library folder
the-library-is-up-to-date = The library is up to date
stopped-early = , stopped early
no-file-needed-moving = No file needed moving
no-file-needed-its-tags-written = No file needed its tags written
the-vault-already-holds-every-track-it = The vault already holds every track it can
delete-that-track = delete that track
delete-those-tracks = delete those tracks
nothing-in-the-inbox-filled-a-want = Nothing in the inbox filled a want
share-that-track = share that track
share-those-tracks = share those tracks
nothing-was-dismissed = Nothing was dismissed
keep-how-long-that-was-heard = keep how long that was heard
unnamed-playlist = playlist { $id }
kept-in-step-with-its-link = { $name } is kept in step with its link
stays-as-it-stands = { $name } stays as it stands
forgot-the-delivered = Forgot the delivered { $title }
started = Started { $name } · { $undo }
started-with = { $count ->
    [one] Started { $name } with { $count } track · { $undo }
   *[other] Started { $name } with { $count } tracks · { $undo }
}
fills-itself = { $name } fills itself, and holds { $held } now
fills-itself-from-the-new-search = { $name } fills itself from the new search, and holds { $held } now
discarded = Discarded { $name } · { $undo }
made = Made { $name } · { $undo }
took-rows-out = { $count ->
    [one] Took { $count } row out
   *[other] Took { $count } rows out
}
wrote-rows = { $count ->
        [one] Wrote { $count } row to { $path } as { $format }
       *[other] Wrote { $count } rows to { $path } as { $format }
    }
moved-rows-into-that-order = { $count ->
    [one] Moved { $count } row into that order
   *[other] Moved { $count } rows into that order
}
kept-in-that-order-and-rows-moved = { $count ->
    [one] Kept in that order, and { $count } row moved into it
   *[other] Kept in that order, and { $count } rows moved into it
}
removed-missing-or-doubled-rows = { $count ->
    [one] Removed { $count } missing or doubled row
   *[other] Removed { $count } missing or doubled rows
}
the-models-edit = The model's edit: { $saying }
gone-again = { $name } is gone again
it-is-again = It is { $name } again
fills-itself-from-the-search-it-had = { $name } fills itself from the search it had
put-back = Put { $name } back
took-back-out-what-went-in = Took back out of { $name } what went in
put-back-in-what-went-out = Put back in { $name } what went out
put-back-in-the-order-it-was-in = Put { $name } back in the order it was in
put-back-the-rows-held-before-refreshed = Put back the rows { $name } held before it was refreshed
started-again = Started { $name } again
fills-itself-from-the-search-it-was-given = { $name } fills itself from the search it was given
put-back-into-what-had-gone-in = Put back into { $name } what had gone in
took-back-out-what-had-gone-out = Took back out of { $name } what had gone out
put-back-in-the-order-that-edit-left = Put { $name } back in the order that edit left
refreshed-from-its-link-again = Refreshed { $name } from its link again
tracks-added = { $count ->
    [one] { $count } track added
   *[other] { $count } tracks added
}
tracks-updated = { $count ->
    [one] { $count } track updated
   *[other] { $count } tracks updated
}
tracks-moved = { $count ->
    [one] { $count } track moved
   *[other] { $count } tracks moved
}
tracks-removed = { $count ->
    [one] { $count } track removed
   *[other] { $count } tracks removed
}
could-not-be-read = { $count } couldn't be read
moved-files = { $count ->
    [one] Moved { $count } file
   *[other] Moved { $count } files
}
could-not-be-moved = { $count } couldn't be moved
wrote-the-tags-into = { $count ->
    [one] Wrote the tags into { $count } file
   *[other] Wrote the tags into { $count } files
}
kept-in-the-vault = { $count ->
    [one] Kept { $count } track in the vault · saved { $saved }
   *[other] Kept { $count } tracks in the vault · saved { $saved }
}
could-not-delete-the-song = Couldn't delete “{ $title }” — its file couldn't be removed
deleted-the-song = Deleted “{ $title }”
could-not-delete-some-songs = Couldn't delete { $kept ->
    [one] { $kept } song
   *[other] { $kept } songs
} of { $asked ->
    [one] { $asked } song
   *[other] { $asked } songs
} — their files couldn't be removed
deleted-songs = { $count ->
    [one] Deleted { $count } song
   *[other] Deleted { $count } songs
}
find-the-identical-songs = find the identical songs
fold-the-identical-songs = fold the identical songs
folded-identical-songs = { $count ->
    [one] Folded { $count } identical song into the song it copies
   *[other] Folded { $count } identical songs into the songs they copy
}
folded-identical-songs-some-kept = { $files ->
    [one] Folded { $files } identical song
   *[other] Folded { $files } identical songs
} — { $kept ->
    [one] { $kept } file couldn't be removed
   *[other] { $kept } files couldn't be removed
}
the-inbox-filled = { $count ->
    [one] The inbox filled { $count } want
   *[other] The inbox filled { $count } wants
}
could-not-be-kept = { $count } couldn't be kept
albums-and-artists-answered = { $albums ->
    [one] { $albums } album
   *[other] { $albums } albums
} and { $artists ->
    [one] { $artists } artist
   *[other] { $artists } artists
} answered
lyrics-for-tracks = { $count ->
    [one] lyrics for { $count } track
   *[other] lyrics for { $count } tracks
}
songs-of-albums-not-held-learnt = { $count ->
    [one] { $count } song of albums not held learnt
   *[other] { $count } songs of albums not held learnt
}
musicbrainz-unreached = MusicBrainz couldn't be reached while looking up { $what } · { $said }
links-on-the-clipboard = { $count ->
    [one] { $count } link are on the clipboard
   *[other] { $count } links are on the clipboard
}
some-links-on-the-clipboard = { $held } of { $asked ->
    [one] { $asked } link
   *[other] { $asked } links
} are on the clipboard
song-by-on-the-clipboard = { $artist } — { $title } is on the clipboard
song-on-the-clipboard = { $title } is on the clipboard
read-in = { $name }: read { $format } as { $encoding }, added { $added }
already-in-it = { $count } already in it
passed-over-naming-no-local-file = passed over { $count } naming no local file
naming-a-file-not-there = { $count } naming a file that is not there
declared-and-not-held = { $count } the sheet declared and did not hold
missing-tracks = { $count ->
    [one] { $count } missing track
   *[other] { $count } missing tracks
}
unheld-releases = { $count ->
    [one] { $count } unheld release
   *[other] { $count } unheld releases
}
brought-back = { $what } brought back
both = { $one } and { $other }
as-its-link-has-it = { $name } is as its link has it
added-songs-to = { $added ->
    [one] Added { $added } song to { $name }
   *[other] Added { $added } songs to { $name }
}
took-songs-out-as-its-link-did = { $removed ->
    [one] Took { $removed } song out of { $name }, as its link did
   *[other] Took { $removed } songs out of { $name }, as its link did
}
added-and-took-out = Added { $added ->
    [one] { $added } song
   *[other] { $added } songs
} to { $name } and took { $removed ->
    [one] { $removed } song
   *[other] { $removed } songs
} out
downloading-more = downloading { $count } more
nothing-went-into = Nothing went into { $name }
put-tracks-in = { $count ->
    [one] Put { $count } track in { $name } · { $undo }
   *[other] Put { $count } tracks in { $name } · { $undo }
}
artists-counted = { $count ->
    [one] { $count } artist
   *[other] { $count } artists
}
albums-counted = { $count ->
    [one] { $count } album
   *[other] { $count } albums
}
tracks-counted = { $count ->
    [one] { $count } track
   *[other] { $count } tracks
}
