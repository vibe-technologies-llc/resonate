no-device-waiting = No speakers or headphones to play through — playing on once one is there
device-stopped-waiting = The audio device stopped playing — playing on once one is there
title-unreadable = Couldn't find “{ $title }” — it may have been moved or deleted
title-unsupported = “{ $title }” is in a format Resonate can't play
title-damaged = Couldn't play “{ $title }” — the file looks damaged
title-cannot-seek = Couldn't jump around in “{ $title }”
song-unopened = That song couldn't be opened
song-cannot-seek = This song can't be skipped through
nothing-playing = Nothing is playing right now
nothing-queued = There's nothing in the queue to play
nothing-to-audition = Play a song first: the test is heard on one playing, and a DSD stream sent as it is cannot be switched within
arms-alike = A and B would sound the same on this stream, so there is nothing to tell apart
levels-unmeasured = A and B could not be measured against each other, so the test cannot level them
levels-too-far-apart = A and B play too far apart in loudness to be matched, so the test cannot level them
file-unreadable = The audio file couldn't be found or read
file-unsupported = The audio file is in a format Resonate can't play
file-damaged = The audio file looks damaged
file-cannot-seek = The audio file can't be skipped through
lookup-unreachable = The online service could not be reached for { $asked }
lookup-refused = The online service refused the request for { $asked } ({ $status })
lookup-unreadable = The online service's answer about { $asked } could not be read
lookup-too-large = The online service's answer about { $asked } was too large to read
lookup-refused-in-a-row = The online service refused { $refusals ->
    [one] { $refusals } request
   *[other] { $refusals } requests
} in a row, the last for { $asked }
lookup-failed = The online lookup of { $asked } failed
measurement-too-large = The { $file } file is larger than { $mebibytes } MiB
measurement-no-points = No line of the { $file } file reads as a frequency and a level
measurement-too-few-points = The { $file } file holds { $found ->
    [one] { $found } point
   *[other] { $found } points
} and a response needs at least { $least }
measurement-misses-the-mid-band = The { $file } spans { $lowest } Hz to { $highest } Hz and must reach 500 Hz to 2 kHz
measurement-nothing-to-correct = The measurement is within { $off_by } dB of the target, so there is nothing to correct
measurement-unreadable = The { $file } file could not be read
sweep-band = A sweep up to 20 kHz needs a rate above 40 kHz and this one runs at { $rate }
sweep-heard-too-short = The recording held { $heard ->
    [one] { $heard } frame
   *[other] { $heard } frames
} and the sweep alone was { $played }, so it was cut short
sweep-heard-too-long = The recording of the sweep was too long to measure
sweep-nothing-heard = Nothing of the sweep was heard: raise the volume or bring the microphone nearer
sweep-unmeasured = The sweep could not be measured
no-device = No speakers or headphones to play through
device-gone = The audio device was unplugged or switched off
sound-server = Lost touch with the sound system — trying again
device-refused = The audio device wouldn't play this song's format
queue-moved = The queue changed before that could happen
player-stopped = The player stopped responding — restart Resonate
unreadable = A song couldn't be found
unsupported = A song is in a format Resonate can't play
damaged = A song couldn't be played
held-by-another-zone = Another zone plays that device bit-perfect
zone-refused = That zone is no longer there
not-handed-over = The other zone didn't take the playback over
nothing-to-compare = The two sides of the listening test can't be told apart here
