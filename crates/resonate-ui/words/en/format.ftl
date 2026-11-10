just-now = just now
now = now
years-ago = { $count ->
    [one] { $count } year ago
   *[other] { $count } years ago
}
months-ago = { $count ->
    [one] { $count } month ago
   *[other] { $count } months ago
}
weeks-ago = { $count ->
    [one] { $count } week ago
   *[other] { $count } weeks ago
}
days-ago = { $count ->
    [one] { $count } day ago
   *[other] { $count } days ago
}
hours-ago = { $count ->
    [one] { $count } hour ago
   *[other] { $count } hours ago
}
minutes-ago = { $count ->
    [one] { $count } minute ago
   *[other] { $count } minutes ago
}
years-briefly = { $count }y
months-briefly = { $count }mo
weeks-briefly = { $count }w
days-briefly = { $count }d
hours-briefly = { $count }h
minutes-briefly = { $count }m
heard-seconds = { $seconds }s
heard-minutes = { $minutes }m
heard-hours = { $hours }h { $minutes }m
depth-16 = 16-bit
depth-24 = 24-bit
depth-32 = 32-bit
depth-32-float = 32-bit float
quality = { $depth } / { $rate }
gain-none = none
gain-kept-under-full-scale = { $landed } dB, to keep the peak under full scale
gain = { $landed } dB
gain-capped = { $landed } dB, capped from { $requested } dB
headroom-unknown = unknown
headroom = { $margin } dB
bit-perfect = Bit-perfect
repacked = Repacked
dithered = Dithered
converted = Converted
bytes = { $count } B
kibibytes = { $amount } KiB
mebibytes = { $amount } MiB
gibibytes = { $amount } GiB
