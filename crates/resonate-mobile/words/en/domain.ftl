sample-rate-out-of-range = A sample rate of { $rate } Hz is outside { $lowest } to { $highest } Hz
channel-count-out-of-range = { $count ->
    [one] { $count } channel
   *[other] { $count } channels
} is outside 1 to { $most }
bit-depth-not-representable = { $bits ->
    [one] { $bits } bit
   *[other] { $bits } bits
} is not a bit depth that can be represented
decibels-not-finite = { $decibels } is not a finite number of decibels
trim-out-of-range = A trim of { $millibels ->
    [one] { $millibels } thousandth
   *[other] { $millibels } thousandths
} of a decibel is outside -{ $most } to { $most }
gain-not-finite = A gain of { $gain } is not a finite, non-negative multiplier
volume-out-of-range = A volume of { $volume } is outside 0 to 1
band-frequency-out-of-range = A band frequency of { $hertz } Hz is outside { $lowest } to { $highest } Hz
band-gain-out-of-range = A band gain of { $decibels } dB is outside -{ $most } to { $most } dB
band-q-out-of-range = A Q of { $q } is outside { $least } to { $most }
preamp-out-of-range = A preamp of { $decibels } dB is outside -{ $most } to { $most } dB
speaker-angle-out-of-range = A speaker angle of { $degrees }° is outside { $least } to { $most }°
speaker-elevation-out-of-range = A speaker elevation of { $degrees }° is outside { $lowest } to { $highest }°
head-yaw-out-of-range = A head turn of { $degrees }° is outside { $rightmost } to { $leftmost }°
zero-id = An identifier cannot be zero
source-name-not-usable = A source is named in lowercase letters, digits, dashes and underscores
not-an-mbid = That is not a MusicBrainz identifier
not-an-isrc = That is not an ISRC
not-a-chromaprint = That is not a Chromaprint written in URL-safe base64
year-not-held = Wrapped does not hold the year { NUMBER($year, useGrouping: "false") }; choose one from { NUMBER($first, useGrouping: "false") } to { NUMBER($last, useGrouping: "false") }
