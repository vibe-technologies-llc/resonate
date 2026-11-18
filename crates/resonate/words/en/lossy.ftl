none-lossy = nothing in { $name } could be heard in a higher quality
held-copy = plays the { $codec } copy held beside it
wanted = wanted, so `resonate poll` fetches a lossless copy
no-release = no higher quality copy held and no release to want
already-waiting = already waiting for a lossless copy
offered = found lossless on { $provider }, so it is downloaded from there
can-be-replaced = { $songs ->
    [one] { $replaceable } of { $songs } song can play a higher quality version now
   *[other] { $replaceable } of { $songs } songs can play a higher quality version now
}
in-playlist = { $name }: { $parts }
replaced = { $songs ->
    [one] { $songs } song now plays the higher quality copy held beside it
   *[other] { $songs } songs now play the higher quality copies held beside them
}
now-wanted = { $songs ->
    [one] wanted { $songs } song, which `resonate poll` fetches in a lossless copy
   *[other] wanted { $songs } songs, which `resonate poll` fetches in lossless copies
}
now-kept = { $songs ->
    [one] queued { $songs } song to download from the service holding it lossless, which the window downloads once it opens
   *[other] queued { $songs } songs to download from the services holding them lossless, which the window downloads once it opens
}
unfiled = { $songs ->
    [one] { $songs } song found lossless on a service needs a music folder to download into
   *[other] { $songs } songs found lossless on a service need a music folder to download into
}
waiting = { $songs } already waiting for a lossless copy
left = left { $songs } with no higher quality copy held and no release to want
nothing-changed = nothing in { $name } could be heard in a higher quality
