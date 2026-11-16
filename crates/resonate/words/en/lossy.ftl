none-lossy = { $name } holds no lossy songs
held-copy = replaced by the { $codec } copy held
wanted = wanted, so `resonate poll` fetches a lossless copy
no-release = no lossless copy held and no release to want
already-waiting = already waiting for a lossless copy
offered = found lossless on { $provider }, so it is downloaded from there
can-be-replaced = { $songs ->
    [one] { $replaceable } of { $songs } lossy song can be replaced now
   *[other] { $replaceable } of { $songs } lossy songs can be replaced now
}
in-playlist = { $name }: { $parts }
replaced = { $songs ->
    [one] replaced { $songs } lossy song with the lossless copy held
   *[other] replaced { $songs } lossy songs with the lossless copies held
}
trashed = { $files ->
    [one] moved { $files } lossy file to the trash
   *[other] moved { $files } lossy files to the trash
}
not-trashed = { $files ->
    [one] { $files } lossy file the trash would not take stays
   *[other] { $files } lossy files the trash would not take stay
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
left = left { $songs } with no lossless copy held and no release to want
nothing-changed = nothing in { $name } could be replaced
