joins = This device joins the devices of { $code }
left = This device left its devices; a new pairing code is made when it next syncs
forgotten = { $device } is off the list of your devices
pairing-code = Pairing code
server = Server
no-server = none, devices on this network only
standing = Standing
on-this-network = on this network
online = online
away = away
this-device = { $where }, this device
lacks-nothing = { $playlist } lacks nothing { $from } could send
came-from = { $landed } of { $missing ->
    [one] { $missing } song
   *[other] { $missing } songs
} came from { $from }
fetch-already-held = { $count ->
    [one] { $count } song was already here
   *[other] { $count } songs were already here
}
fetch-claimed = { $count ->
    [one] { $count } song was already being fetched by another transfer
   *[other] { $count } songs were already being fetched by another transfer
}
fetch-unreachable = { $count ->
    [one] { $from } could not be reached for { $count } song
   *[other] { $from } could not be reached for { $count } songs
}
fetch-failed = { $count ->
    [one] { $count } song failed to arrive
   *[other] { $count } songs failed to arrive
}
fetch-unanswered = { $count ->
    [one] { $count } song had no answer in time
   *[other] { $count } songs had no answer in time
}
zone = Zone
state = State
playing = Playing
volume = Volume
percent = { $percent }%
queue = Queue
songs = { $songs ->
    [one] { $songs } song
   *[other] { $songs } songs
}
billed = { $title } — { $artist }
idle = idle
playing-now = playing
paused = paused
stopped = stopped
connecting = connecting
connected = connected
unreachable = the server could not be reached
refused = the server refused this device
nearby = syncing with devices on this network
