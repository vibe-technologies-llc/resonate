every-other-device = every other device
its-own-curve = its own curve
unreadable = unreadable
measured = measured
switched = equaliser: { $state }
on = on
off = off
own-curve-of = the own curve of { $owner }
bound = bound { $binding } to { $sink }
unbound = unbound { $sink }
shaped-from = shaped { $own } from { $from }, { $bands ->
    [one] { $bands } band
   *[other] { $bands } bands
}
kept-graphic = kept a graphic curve as { $name }, { $bands ->
    [one] { $bands } band
   *[other] { $bands } bands
} at { $rate } and fitted again at the rate a stream plays at
kept = kept { $name }, { $bands ->
    [one] { $bands } band
   *[other] { $bands } bands
}
the-layout-file = the { $layout } { $file }
fitted-toward = { $bands ->
    [one] { $bands } band
   *[other] { $bands } bands
} fitted toward { $target }, { $before } dB off before and { $after } dB after
shaped-from-told = shaped { $own } from { $origin }: { $told }
kept-from-told = kept { $name } from { $origin }: { $told }
what-the-desktop-plays = what the desktop plays
the-default-microphone = the default microphone
the-default-output = the default output
playing-a-sweep = playing a sweep through { $through } and listening to { $heard } for { $seconds } s
wrote-the-measurement = wrote the measurement to { $path }
the-sweep = the sweep it measured
nothing-measured = nothing measured answers to that
no-correction = { $device }: no correction is held
not-fetched = { $device }: it could not be fetched
kept-corrections = kept { $kept } of { $asked ->
    [one] { $asked } correction
   *[other] { $asked } corrections
}
measurement-passed-over = { $lines ->
    [one] { $lines } line of the measurement was passed over
   *[other] { $lines } lines of the measurement were passed over
}
replaced = { $name } was kept already and is replaced
lines-passed-over = { $lines ->
    [one] { $lines } line was passed over
   *[other] { $lines } lines were passed over
}
filters-approximated = { $filters ->
    [one] { $filters } filter names a rolloff this build reads as second order
   *[other] { $filters } filters name a rolloff this build reads as second order
}
values-clamped = { $values ->
    [one] { $values } value past what this build holds was held to its limits
   *[other] { $values } values past what this build holds were held to its limits
}
wrote = wrote { $spoken } to { $path }
forgot = forgot { $name }
no-profiles = no profiles are kept in { $folder }
own-curve-heading = OWN CURVE OF
bands-heading = BANDS
preamp-heading = PREAMP
bound-heading = BOUND
yes = yes
profile-heading = PROFILE
bound-to-heading = BOUND TO
device-heading = DEVICE
nothing-bound = nothing is bound to any device
bound-but-not-kept = { $spoken } is bound but no profile of that name is kept
for-sink = for { $sink }: { $spoken }
preamp = preamp: { $preamp }
fitted-to-graphic = fitted to a graphic curve of { $points } points: the bands below are its fit at { $rate }, and a stream at another rate is fitted again at its own
holds-no-bands = it holds no bands
type-heading = TYPE
frequency-heading = FREQUENCY
gain-heading = GAIN
peak = peak: { $peak } dB
measured-by-heading = MEASURED BY
rig-heading = RIG
path-heading = PATH
measurer-missed = { $asked } measured nothing for it: { $chosen }'s is the best ranked
sink = sink:   { $description } ({ $name })
looks-like = looks like: { $device }
fetch-it = fetch it:   resonate eq --fetch "{ $device }" --for { $sink }
not-clear-enough = nothing measured answers to that name clearly enough to be worth guessing at
