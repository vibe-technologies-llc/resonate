bound-to-note = Each device plays through what is bound to it here — nothing, a curve of its own drawn under Bands, or a kept profile — and a device bound to nothing takes what every other device does.
the-curve = Equaliser curve
fit-toward-hint = What a measurement is fitted toward
a-file-as-the-target = A file…
heard-through-hint = What hears the sweep: the desktop measures the path to the output and no room, a microphone measures the room
desktop-heard = The desktop
default-microphone-heard = The default microphone
no-file-picker = Couldn't open the file picker
not-bit-perfect = On, the samples reaching the device are not the file's own: the playback bar reads converted rather than bit-perfect, and a DSD stream packed as DoP is left untouched.
nothing-bound = Nothing is bound to this device, so nothing is applied. Press the curve to give it a curve of its own, bind a kept profile above, import an EqualizerAPO file, or fetch the correction measured for what is plugged in.
shaped-by-hand = Press the curve to add a band, drag a handle to move it, scroll over one to change its Q, and right-press it to take it out.
no-measuring = This build cannot play a sweep and hear it
uncalibrated = None chosen: the fit corrects the microphone too
autoeq-note = Who measured each result and on which rig is said beside it; a correction used is fetched once and kept here as an ordinary profile.
measurers-note = Where several measured one device, the measurer named here is taken before the fixed ranking, in the search and in what is suggested for what is plugged in.
measure-note = Plays a sweep of eight seconds through the output in use, straight to the device and past the equaliser, and records it for two more; Stop ends it and keeps nothing. Keep the room quiet. The response is fitted toward the target above and kept as the profile named, measured when none is. The microphone's own response is corrected too unless you choose its calibration, a measurement file taken off what is heard.
own-measurement-note = Choose a frequency-response measurement of your own — AutoEq's CSV, REW's exported text, or two columns of frequency and decibels — and a target to fit it toward. Both are set level across the middle of the spectrum and smoothed to a sixth of an octave, and up to ten bands, shelves among them, are fitted to what lies between them and kept here as a profile with its preamp.
follows-the-default = Follows the default
bound-to-nothing = Nothing
not-plugged-in = A device not plugged in now
its-own-curve = Own curve
every-other-device = Every other device
apply-the-equaliser = Apply the equaliser
off-the-samples-reach-the-device = Off, the samples reach the device as the file wrote them
nothing-is-bound-to-any-device = Nothing is bound to any device yet: press the curve below to shape one
the-binding-a-device-that-names = The binding a device that names none of its own falls back to
fit = Fit
set-the-preamp-from-the-curves = Set the preamp from the curve's own peak, then from what the songs in the queue peak at through it, so the loudest of them reaches full scale and no further
take-this-band-out = Take this band out
what-shape-this-band-gives-the = What shape this band gives the curve
add-a-band = Add a band
import = Import…
export = Export…
keep-as-a-profile = Keep as a profile
press-again-to-discard = Press again to discard
discard = Discard
nothing-is-reached-turn-online-on = Nothing is reached: turn Online on, or this build carries no network.
asking-autoeq = Asking AutoEq…
fit-toward = Fit toward
fitting = Fitting…
fit-a-measurement = Fit a measurement…
heard-through = Heard through
microphone-calibration = Microphone calibration
choose-a-calibration = Choose a calibration…
remove = Remove
keep-as = Keep as
measuring = Measuring…
measure = Measure
stop = Stop
calibrate-with = Calibrate with
keep-a-profile-under-that-name = keep a profile under that name
choose = Choose
best-ranked = Best ranked
whose-correction-to-prefer-where-several = Whose correction to prefer where several measured a device. Best ranked leaves it to the fixed ranking.
for-every-device = For every device
best-ranked-leaves-it-to-the = Best ranked leaves it to the fixed ranking
as-for-the-rest = As for the rest
whose-correction-to-prefer-for-this = Whose correction to prefer for this device. As for the rest takes the one chosen for every device, else the fixed ranking.
as-for-the-rest-takes-the = As for the rest takes the one chosen for every device
use-this = Use this
fetch-this-measurement-and-keep-it = Fetch this measurement and keep it here as a profile
what-is-plugged-in = What is plugged in
look-it-up = Look it up
read-autoeqs-list-of-measured-devices = Read AutoEq's list of measured devices and weigh this device's own name against it
nothing-measured-answers-to-that-name = Nothing measured answers to that name clearly enough to be worth guessing at. Search above if you know what is plugged in.
off = , off
every-channel = Every channel
every-other-channel = Every other channel
left = Left
right = Right
centre = Centre
subwoofer = Subwoofer
rear-left = Rear left
rear-right = Rear right
side-left = Side left
side-right = Side right
a-channel = A channel
import-2 = Import
room-note = The response is convolved with every stream after the equaliser, at the stream's own rate, a partition of 1 024 frames behind. It is read again from the file at every start, so a new measurement written over it is heard from then on. A four-channel file is read as a two-by-two (binaural) response, in the order left-to-left, left-to-right, right-to-left, right-to-right; two stereo files, one for each speaker, are joined into one. A SOFA set of head-related responses is read as the same, from the measurements nearest the speaker angle, blended where several lie close; a netCDF file of that shape reads as well, and so does a set of speakers measured at each ear.
speaker-note = How far to either side of the front the speakers stand, from 5 to 90 degrees, for a SOFA set. Changing it reads the set again.
elevation-note = How far above the level of the ears the speakers stand (below, when negative), from -30 to 60 degrees. Where the set's measurements surround that direction, the response is interpolated between the corners of the triangle that holds it.
yaw-note = How far your head is turned from the front, from -90 to 90 degrees, to the left when positive. The speakers are looked up that many degrees round the other way, and in a set of speakers measured at several head orientations the view nearest the turn is read.
binaural-note = A two-by-two (binaural) response: each speaker's response is heard at both ears, so a stereo stream is convolved four ways.
no-room = Nothing is corrected.
choose-a-response = Choose a response…
choose-a-pair = Choose a pair…
stop-correcting = Stop correcting
bring-the-speakers-a-degree-closer = Bring the speakers a degree closer
put-the-speakers-a-degree-further = Put the speakers a degree further apart
speaker-angle = Speaker angle
lower-the-speakers-a-degree = Lower the speakers a degree
raise-the-speakers-a-degree = Raise the speakers a degree
speaker-elevation = Speaker elevation
turn-the-head-a-degree-to = Turn the head a degree to the right
turn-the-head-a-degree-to-2 = Turn the head a degree to the left
head-turned = Head turned
left-speaker = Left speaker
right-speaker = Right speaker
preamp-fitted-to = { $count ->
    [0] The preamp is fitted to the room correction
    [one] The preamp is fitted to the one song measured
   *[other] The preamp is fitted to the { $count } songs measured
}
kept-not = The pair could not be kept as one response
the-two-files-are-at-different = The two files are at different sample rates
the-two-files-are-of-different = The two files are of different lengths
the-two-files-could-not-be = The two files could not be joined into one response
correcting = The room is corrected from now on
no-taps = That file holds no response to convolve with
unread = That file could not be read as a response
band-keys-hint = Choose this band on the curve. A semitone lower or higher — { $lower } or { $higher }. Half a decibel quieter or louder — { $quieter } or { $louder }. Narrower or wider — { $narrower } or { $wider }
reaches-before-the-preamp = Reaches { $peak } dB before the preamp
for-the-device = For { $description }
curve-drawn = Gain against frequency, drawn within { $widest } dB either side of flat, shaped by { $count ->
    [one] { $count } band
   *[other] { $count } bands
}
kilohertz = { $kilohertz } kHz
hertz = { $hertz } Hz
band-gain = , { $gain } dB
band-said = { $kind } at { $at }{ $gain }{ $off }
speakers-either-side = Speakers { $degrees } degrees either side of the front
speakers-above-the-ears = Speakers { $degrees } degrees above the ears
head-turned-by = Head turned { $degrees } degrees
speaker-unread = { $speaker ->
    [left] The left speaker's file could not be read as a response
   *[right] The right speaker's file could not be read as a response
}
speaker-empty = { $speaker ->
    [left] The left speaker's file holds no response to convolve with
   *[right] The right speaker's file holds no response to convolve with
}
speaker-not-stereo = { $speaker ->
    [left] The left speaker's file is not stereo
   *[right] The right speaker's file is not stereo
}
speaker-already-crossed = { $speaker ->
    [left] The left speaker's file is already a two-by-two response
   *[right] The right speaker's file is already a two-by-two response
}
