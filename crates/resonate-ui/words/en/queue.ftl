remove-hint = Take this row out of the queue
remove-reached-hint = Take every reached row out of the queue
queued-hint = Put every queued row in a playlist
clear-hint = Take every row out of the queue and stop
history = History
now-playing = Now playing
playing-next = Playing next
continue-playing = Continue playing
empty = The queue is empty.
how-to-fill = Play something from Albums or Tracks, or add rows with the queue marks.
tracks = { $count ->
    [one] { $count } track
   *[other] { $count } tracks
}
at-least = at least { $length }
on-row = on row { $row }
waiting = { $count } to play next
continue-from = Continue from { $device }
continue-from-hint = Play the { $songs } songs { $device } was playing, from the one it was on
queue = Queue
clear = Clear
save-as-a-playlist = Save as a playlist
put-back = Put back
take-out-again = Take out again
took-out = { $count ->
    [one] Took { $count } track out of the queue
   *[other] Took { $count } tracks out of the queue
}
puts-back = Put the rows last taken out of the queue back where they were. { $behind ->
    [0] It is the last one there is to put back
    [one] One more gesture is behind it
   *[other] More gestures are behind it
}
takes-out-again = Take the rows last put back out of the queue again. { $behind ->
    [0] It is the last one there is to take out again
    [one] One more put back is behind it
   *[other] More put backs are behind it
}
suggested-from = Suggested · { $service }
download-song-hint = Download this suggested song to your library
downloaded-song-hint = Queued for download
download-to-library = Download to library
mix-in-suggestions = Mix in suggestions
mix-in-suggestions-hint = Weave suggested streamed songs into the upcoming queue
mixed-in-suggestions = { $count ->
    [one] Mixed { $count } suggested song into the queue
   *[other] Mixed { $count } suggested songs into the queue
}
no-queue-suggestions = No new suggested songs were found for this queue

