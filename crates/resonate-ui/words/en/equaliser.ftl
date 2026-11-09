this-devices-own-curve = This device's own curve
every-other-devices-own-curve = Every other device's own curve
own-curve-file = own curve.txt
measurement-lines-passed-over = { $lines ->
    [one] ; { $lines } line of the measurement was passed over
   *[other] ; { $lines } lines of the measurement were passed over
}
fitted = Kept { $name }, { $bands ->
    [one] { $bands } band
   *[other] { $bands } bands
} fitted toward { $aimed_at }: { $before } dB off before and { $after } dB after
measured = measured
measurement-stopped = The measurement was stopped; nothing was kept
nothing-heard = Nothing was heard — check the microphone is on and the output can be heard
no-output-for-the-sweep = The output to play the sweep through isn't there
sweep-unheard = Couldn't play the sweep and hear it
fit-that-measurement = fit that measurement
lines-passed-over = { $count ->
    [one] ; { $count } line this build does not read was passed over
   *[other] ; { $count } lines this build does not read were passed over
}
rolloffs-approximated = { $count ->
    [one] ; { $count } filter names a rolloff it reads as second order
   *[other] ; { $count } filters name a rolloff it reads as second order
}
values-clamped = { $count ->
    [one] ; { $count } value past what it holds was held to its limits
   *[other] ; { $count } values past what it holds were held to its limits
}
bound-but-not-kept = { $curve } is bound but not kept here
read-that-profile = read that profile
save-the-curve = save the curve
carry-the-curve = carry the curve to the renamed device
bands-at-most = a curve holds at most { $most } bands
kept-graphic = kept a graphic curve as { $name }, fitted to { $bands ->
    [one] { $bands } band
   *[other] { $bands } bands
} here and again at the rate each stream plays at
kept-with-bands = Kept { $name }, { $bands ->
    [one] { $bands } band
   *[other] { $bands } bands
}
import-that-profile = import that profile
wrote-out = Wrote { $name } out
export-the-profile = export the profile
kept-the-curve-as = Kept the curve as { $name }
keep-as-a-profile = keep the curve as a profile
discarded = Discarded { $name }
discarded-the-own-curve = Discarded the device's own curve
discard-that-curve = discard that curve
read-the-catalogue = read the AutoEq catalogue
kept-from-autoeq = kept { $name }, { $bands ->
    [one] { $bands } band
   *[other] { $bands } bands
}
no-autoeq-measurement = AutoEq has no measurement for that device
frequency-refused = a band sits between 1 Hz and 40 kHz
gain-refused = a band's gain sits between -40 and +40 dB
q-refused = a Q sits between 0.10 and 40.00
