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
