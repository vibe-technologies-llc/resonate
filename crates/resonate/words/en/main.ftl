just-now = just now
cover-art = cover art
taken-away = (taken away)
rating = rating
unknown = unknown
none = none
the-network = the network
a-device = a device
the-graph = the graph
nothing-plugged-in = { $port } (nothing plugged in)
plugged-in = { $port } (plugged in)
volume-in-software = { $port }, volume in software
volume-on-the-hardware = { $port }, volume on the hardware
volume-always-on-the-device = { $port }, volume always on the device
heard-at = { $port } at { $level }
muted = { $port }, muted
scanned = discovered { $discovered } | added { $added } | updated { $updated } | moved { $moved } | removed { $removed } | failed { $failed }
cancelled = cancelled
carrying-on = carrying on the lookup a run left unfinished
stopped-unreached = stopped: the reference could not be reached while asking for { $asked }
nothing-asked = nothing was asked
asked-release = a release
asked-find-release = a release search
asked-recording = a recording
asked-isrc = a recording by its ISRC
asked-find-recording = a recording search
asked-release-group = a release group
asked-find-release-group = a release group search
asked-artist = an artist
asked-find-artist = an artist search
asked-release-groups-of-artist = the releases of an artist
asked-releases-of-group = the pressings of a release
asked-releases-of-artist = the pressings of an artist
asked-cover = a cover
asked-portrait = a portrait
asked-lyrics = lyrics
asked-devices = the measured devices
asked-correction = a measured correction
asked-recognise = a recognition
asked-submit = a submission of what was heard
asked-love = a favourite told to a listening service
asked-token = a check of a listening service's token
asked-stream-link = a look for where a track streams
asked-follow-link = what a link names
asked-share-file = a public file upload
asked-latest-release = the latest release of Resonate
asked-spotify-account = a sign-in to the Spotify account
nothing-wanted = nothing is wanted
held = held
gave-up = gave up after { $tries ->
    [one] { $tries } try
   *[other] { $tries } tries
}
again-in = { $standing }, again in { $left }
due-again = { $standing }, due
offered = offered
due = due
unanswered = unanswered
no-match = no match { $misses } of { $tries }
lasting-minutes = { $minutes }m
lasting-hours = { $hours }h
lasting-hours-and-minutes = { $hours }h { $minutes }m
read-the-rest = read { $releases ->
    [one] { $releases } release
   *[other] { $releases } releases
} more of { $artist }'s discography
dismissed-beside = { $dismissed } dismissed, --bring-back lists them again
brought-back = brought back { $tracks ->
    [one] { $tracks } dismissed track
   *[other] { $tracks } dismissed tracks
} and { $releases ->
    [one] { $releases } dismissed release
   *[other] { $releases } dismissed releases
}
nothing-missing = nothing is missing
nothing-of-missing = nothing of { $artist } is missing
missing-across = { $tracks ->
    [one] { $tracks } track
   *[other] { $tracks } tracks
} missing across { $albums ->
    [one] { $albums } album
   *[other] { $albums } albums
} · { $releases ->
    [one] { $releases } release
   *[other] { $releases } releases
} not held
tracks-missing = TRACKS MISSING
every-track-held = every release track the catalog knows of is held
releases-not-held = RELEASES NOT HELD
every-release-held = every release of a held artist is held
credits-unread = MusicBrainz credits { $artist } on { $groups ->
    [one] { $groups } release group
   *[other] { $groups } release groups
} more that were not read
yes = yes
no-provider = no provider is registered
polled = asked { $asked } | offered { $offered } | kept { $kept } | unkept { $unkept } | nothing { $nothing } | refused { $refused } | late { $late }
forgot = forgot { $root }
forgot-delivered = forgot the delivered { $path }
forgot-gone = forgot { $tracks ->
    [one] { $tracks } track
   *[other] { $tracks } tracks
} gone from { $folder }
forgot-nothing = { $path } was neither a library root, a delivered track nor a folder holding a track that is gone
copies = { $copy } copies { $kept }
differs = { $path } is numbered like a copy but differs, so it stays
to-fold = { $copies ->
    [one] { $copies } copy
   *[other] { $copies } copies
} to fold; --apply folds them
folded = { $copies ->
    [one] { $copies } copy
   *[other] { $copies } copies
} folded and sent to the trash
trash-refused = { $kept ->
    [one] { $kept } the trash would not take, so it stays
   *[other] { $kept } the trash would not take, so they stay
}
plays = { $plays ->
    [one] { $plays } play
   *[other] { $plays } plays
}
favourite-plays = favourite · { $plays }
not-favourite-plays = not a favourite · { $plays }
nothing-to-tag = nothing to tag
sheet-track = track { $number } { $field }
written = written { $written } | fields { $fields } | pictures { $pictures } | ratings { $ratings }
would-write = would write { $written } | fields { $fields } | pictures { $pictures } | ratings { $ratings }
tagged = { $written } | already said { $unchanged } | passed over { $passed } | walked { $walked }
nothing-written = nothing was written; pass --apply to do it
nothing-to-put-back = no applied run is kept to put back
nothing-to-organise = nothing to organise
moved = moved { $moved }
folders-pruned = folders pruned { $folders }
would-move = would move { $moved }
folders-to-empty = folders to empty { $folders }
organised = { $moved } | in place { $unchanged } | unidentified { $unidentified } | collided { $collided } | failed { $failed } | { $folders }
nothing-moved = nothing was moved; pass --apply to do it
refused-with = { $standing }: { $why } { $with }
refused-kind = { $standing }: { $why } ({ $kind })
refused = { $standing }: { $why }
unidentified = nothing names an album to file it under
loose = it would land loose in the root
collided = it would collide with
shares-a-sheet = it would be split from another file named by
named-from-above = it is cut by a sheet in a folder above it,
out-of-reach = its path would be longer than Windows takes
source-gone = it is no longer where the catalog says
unmoved = the volume refused the move
explain-source = source: { $spec }
explain-dsd = dsd:    { $rate } ({ $hertz } Hz, 1-bit)
explain-sink = sink:   { $description } ({ $name })
explain-output = output: { $stream } [{ $mode }]
explain-words = words:  offered as { $offered }; the device names { $named }
explain-remix = remix:    { $from } -> { $to }
explain-resample = resample: { $from } -> { $to } [{ $quality }, { $phase } phase]
explain-eq = eq:      { $bands ->
    [one] { $bands } band
   *[other] { $bands } bands
}, peak { $peak } dB, preamp { $preamp }
explain-eq-unbound = eq:      on, but nothing this device is bound to shapes the sound
explain-room = room:     { $taps } taps a channel over { $channels } channels, measured at { $rate }
found-as-it-plays = found as it plays
studied-at = studied at { $kilohertz } kHz
explain-restore = restore: { $restoration } a { $tuning } source, its wall { $wall }
explain-crossfeed = crossfeed: { $crossfeed }
explain-guard = guard:   true peaks held under -0.1 dBTP
explain-dither = dither:  to { $depth } [{ $shaping }]
explain-dop = dop:     { $reading }
above-nyquist = { $bands ->
    [one] { $bands } band is above the output's Nyquist and is not applied
   *[other] { $bands } bands are above the output's Nyquist and are not applied
}
dop-packed = packed over PCM, untouched
dop-off = decimated to PCM; dop is off
dop-inexact = decimated to PCM; the sink does not take the carrier exactly
dop-gain = decimated to PCM; a gain stage would rewrite the markers
a-list-kept-in = a list kept in { $order }
a-list = a list
itself-with = itself, { $text }
itself = itself
in-order = { $order } order
in-reverse-order = reverse { $order } order
by-album = album
by-artist = artist
by-title = title
by-length = length
by-file-name = file name
minutes-ago = { $minutes }m ago
hours-ago = { $hours }h ago
days-ago = { $days }d ago
renamed = { $name } is called { $into } now
exported = wrote { $rows } rows of { $name } to { $path } as { $format }
back-in-hand = { $name } is back in hand, and a row added to it lands at the end
holds-nothing = { $name } holds nothing to play
copied = copied { $copied } of { $name } into { $target }, which now holds { $held }
query-revised = { $name } now fills itself{ $matching }, and holds { $held } now
query-saved = { $name } fills itself{ $matching }, and holds { $held } now
reads-as = it reads that as { $reads }
whatever-matches = with whatever matches { $text }
the-whole-library = with the whole library
added = added { $added } to { $name }, which now holds { $held }
imported = { $name }: read { $format } as { $encoding }, added { $added }, which now holds { $held }
already-in-order = { $name } was already in that order
reordered = { $moved ->
    [one] moved { $moved } row of { $name } into that order
   *[other] moved { $moved } rows of { $name } into that order
}
kept-already = { $name } is kept in { $order }, and was already in it
now-kept = { $moved ->
    [one] { $name } is kept in { $order }, and { $moved } row moved into it
   *[other] { $name } is kept in { $order }, and { $moved } rows moved into it
}
all-there = every row of { $name } still names a file that is there
tidied = { $dropped ->
    [one] dropped { $dropped } row of { $name } whose file has gone
   *[other] dropped { $dropped } rows of { $name } whose files have gone
}
none-doubled = every row of { $name } names a file no other row does
folded-doubles = { $dropped ->
    [one] folded { $dropped } doubled row of { $name } into the one above it
   *[other] folded { $dropped } doubled rows of { $name } into the ones above them
}
pinned = { $name } is pinned, and stands at the top of the listing
unpinned = { $name } is unpinned, and takes its place in the order the listing reads in
discarded-search = discarded { $name } and the search it filled itself from
discarded-empty = discarded { $name }, which held nothing
discarded = { $held ->
    [one] discarded { $name } and the { $held } row it held; the file stays where it is
   *[other] discarded { $name } and the { $held } rows it held; the files stay where they are
}
none-match = nothing in { $name } matches { $text }, so no row was taken out
took-out = { $dropped ->
    [one] took { $dropped } row matching { $text } out of { $name }
   *[other] took { $dropped } rows matching { $text } out of { $name }
}
already-in-it = { $already ->
    [one] { $already } was already in it
   *[other] { $already } were already in it
}
passed-over = passed over { $passed } naming no local file
fell-short = { $short ->
    [one] the sheet declared { $short } row it did not hold
   *[other] the sheet declared { $short } rows it did not hold
}
not-on-disk = { $missing ->
    [one] { $missing } names a file that is not there
   *[other] { $missing } name files that are not there
}
queued-onto = queued { $queued } onto { $player }
moving-to = moving what { $player } plays to zone { $zone }
no-player = no player of this build is running
in-the-background = playing in the background, so no key is read; fg and an interrupt stop it
unknown-key = unknown key { $key }; ? for help
started = started  { $track }
finished = finished { $track }
output = output   { $stream } [{ $mode }] on sink { $sink }
underrun = underrun { $missing } frames
failed = { $track } failed
waits = { $track } waits for a device
rejected = { $command } rejected
untuned = the window already open did not take the settings given for this run
handed = handed to the window already open as { $window }
