on = { $title } on { $album }
wanted = wanted { $shown }
dismissed = dismissed { $shown }
tracks-dismissed = { $tracks ->
    [one] { $tracks } missing track
   *[other] { $tracks } missing tracks
} dismissed, --bring-back lists them again
dismissed-by = dismissed { $title } by { $artist }
releases-dismissed = { $releases ->
    [one] { $releases } release
   *[other] { $releases } releases
} dismissed, --bring-back lists them again
no-longer-wanted = no longer wanted: { $shown }
