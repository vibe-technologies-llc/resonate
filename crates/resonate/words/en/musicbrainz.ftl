removed = The copy of MusicBrainz is removed.
none-was-held = No copy of MusicBrainz was held.
built = The copy of MusicBrainz is built at { $path } ({ $gigabytes } GB) in { $minutes ->
    [one] { $minutes } minute
   *[other] { $minutes } minutes
}.
mirror-off = musicbrainz-mirror is off, so lookups do not ask it until it is turned on.
none-held = No copy of MusicBrainz is held. `resonate musicbrainz --build` makes one.
found = { $title } by { $credited }
held = The export of { $dump } is held at { $path } ({ $gigabytes } GB).
held-though-off = The export of { $dump } is held at { $path } ({ $gigabytes } GB), though musicbrainz-mirror is off.
answers-kept = { $answers ->
    [one] { $answers } lookup answer is kept to share with synced devices.
   *[other] { $answers } lookup answers are kept to share with synced devices.
}
replicated = The copy of MusicBrainz follows the changes up to packet { $to } ({ $changes ->
    [one] { $changes } change
   *[other] { $changes } changes
}, { $documents ->
    [one] { $documents } entry rewritten
   *[other] { $documents } entries rewritten
}).
already-replicated = The copy of MusicBrainz already holds every change published, up to packet { $to }.
amended = It follows MusicBrainz's changes up to packet { $sequence }, applied { $ago }.
