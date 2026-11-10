problem = Problem
done = Done
note = Note
reads = Reads
plays = { $count ->
    [one] { $count } play
   *[other] { $count } plays
}
title = Title
artist = Artist
format = Format
heard = Heard
length = Length
sort-by-number = Sort by album order
sort-by-marked = Sort by when it was marked a favourite
sort-by-title = Sort by title
sort-by-artist = Sort by artist
sort-by-format = Sort by format, lossless and high resolution first
sort-by-heard = Sort by how often it is heard
sort-by-length = Sort by length
