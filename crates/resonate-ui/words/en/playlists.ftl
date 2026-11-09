picker-label = Add to a playlist
share-with-people = Share with other people…
pick-hint = Add to this playlist
new-hint = Start a playlist, and name it
play-hint = Play this playlist in order from the top
shuffle-hint = Play this playlist, shuffled
no-file-picker = Couldn't open the file picker
next-hint = Hear this straight after the track playing
queue-hint = Queue this after what is already queued, ahead of the rest of what is playing
playlist-next-hint = Hear this playlist straight after the track playing
playlist-queue-hint = Queue this playlist after what is already queued, ahead of the rest of what is playing
rename-hint = Give this playlist another name
refresh-hint = Read it again from its link now: a playlist kept in step takes the service's songs and order, one imported once only gains what it lacks, and songs you do not have are downloaded
pin-hint = Keep this playlist at the top of the list, whichever order it is read in, and in the sidebar
unpin-hint = Let this playlist sit where the order puts it
discard-hint = Forget this playlist. The files stay where they are
back-hint = Back to every playlist
import-hint = Read playlists in from M3U, PLS or XSPF files
tidy-hint = Drop rows whose files are gone and repeated rows, keeping the first of each file
sort-hint = Put every row in order at once, rather than moving them one at a time, or keep the list in that order. A row no scan has seen goes to the end, except under file name, which reads the path every row carries
once-hint = Put the rows in order now, and leave a row added later at the end
always-hint = Keep the list in that order, so a row added later lands where the order puts it rather than at the end
narrowed-hint = A search is narrowing this playlist, so the rows beside one are not here for it to move between: clear the search to put them in order by hand again. Play, Play next, Add to queue and Drop shown take the rows shown; Sort, Tidy and Export take the playlist whole, however little of it a search has left on screen
kept-hint = This playlist is kept in order, so a row lands where the order puts it rather than where it is dropped. Press Sort to read it another way round, or to put it back in hand
fills-itself-hint = This playlist is a search: it holds whatever the library matches now, so nothing in it is a row to move or drop. Press to change what it looks for
save-search-hint = Keep this search as a playlist that fills itself as the library grows
add-hint = Put this track in a playlist
add-reached-hint = Put every reached row in a playlist
add-songs-hint = Choose songs from the library to add to this playlist
add-song-hint = Add this track to the playlist
finish-adding-hint = Return to the playlist
open-hint = Open this playlist
more-hint = Queue, rename, duplicate, export or discard this playlist
drop-shown-hint = Take every row shown here out of the playlist. The rows the search does not match stay, and the files stay where they are
remove-hint = Take this row out of the playlist
remove-reached-hint = Take every reached row out of the playlist
search-order-hint = What this search puts its rows in order by
grid-hint = Draw the playlists as a grid of covers, the way albums are
list-hint = Draw the playlists as a list, with how often and how lately each was played
just-once = Just once
from-now-on = From now on
no-playlists-match = No playlists match
none-named-that = No playlist is named that. A playlist answers a search by its name alone.
no-playlists-yet = No playlists yet
how-to-start = Start one here, read one in from another player, or press + on any track or queue row.
new-playlist = New playlist
import = Import
grid = Grid
list = List
playlists = Playlists
name-it = Name it
name-this-search = Name this search
change-this-search = Change this search
rename-it = Rename it
order = Order
rows = Rows
no-songs-match = No songs match
nothing-here-matches = Nothing in this playlist matches the search.
nothing-matches-yet = Nothing matches yet
nothing-matches-the-search-yet = Nothing in the library matches this search. It fills itself as the library grows, or its search can be changed.
edit-search = Edit search
this-playlist-is-empty = This playlist is empty
how-to-fill = Choose songs from the library, or press + on any track or queue row.
add-songs = Add songs
fills-itself = fills itself from a search
kept-in-order = kept in order
narrowed-by-the-search = narrowed by the search
saved-search = Saved search
playlist = Playlist
play = Play
shuffle = Shuffle
drop-shown = Drop shown
all-playlists = all-playlists
keep-this-order = Keep this order
or-a-new-one = Or a new one
a-new-one = New playlist
undo = Undo
redo = Redo
shared = Shared
followed = Followed
pinned = Pinned
search = Search
kept = Kept
stop-keeping-its-songs = Stop keeping its songs here
keep-its-songs-here = Keep its songs here
songs-no-longer-fetched = Its songs are no longer fetched by themselves
songs-fetched = Its songs are fetched from your other devices while they are online
copy-into = { $count ->
    [one] Copy this track into
   *[other] Copy { $count } tracks into
}
add-to = { $count ->
    [one] Add this track to
   *[other] Add { $count } tracks to
}
playlists-counted = { $count ->
    [one] { $count } playlist
   *[other] { $count } playlists
}
cap-is-none-of-these = { $count } rows is the cap, which is none of these.
unnamed = playlist { $id }
keep-in-step = Keep in step with { $service }
keep-in-step-hint = Imported once from { $service }. Keep it in step: read the link again every six hours while the window is open, taking the songs and order { $service } has and leaving out what it took out
in-step = In step with { $service }
in-step-hint = Kept in step with { $service }, read again every six hours. Press to leave it as it stands from now on
shared-about = Shared · { $about }
followed-about = Followed · { $about }
made-and-last-played = Made { $made } · last played { $played }
made-and-never-played = Made { $made } · never played
tracks = { $count ->
    [one] { $count } track
   *[other] { $count } tracks
}
tracks-played = { $tracks } · played { $played }
fetch-missing = Fetch { $missing } missing songs from { $device }
on-their-way = Songs are on their way from { $device }
imported-from = imported from { $service }
kept-in-step-with = kept in step with { $service }
undo-started = Take { $name } away again, as though it had never been started
undo-renamed = Name it { $name } again
undo-revised = Put back the search { $name } filled itself from
undo-discarded = Put { $name } back, with everything it held
undo-added = Take back out of { $name } what that edit put in
undo-removed = Put back in { $name } what that edit took out
undo-moved = Put the rows of { $name } back in the order they were in
undo-refreshed = Put back the rows { $name } held before it was refreshed from its link
redo-started = Start { $name } again, with everything it held
redo-renamed = Name it { $name } again
redo-revised = Put back the search { $name } was given
redo-discarded = Take { $name } away again, with everything it holds
redo-added = Put back into { $name } what that edit had put in
redo-removed = Take back out of { $name } what that edit had taken out
redo-moved = Put the rows of { $name } back in the order that edit left them in
redo-refreshed = Refresh { $name } from its link again, as it was refreshed
made-by-a-model = A model made this edit, in a session beside the window.{" "}
puts-back = { $doing }. { $made-by }Undo — { $keys }. Each press walks back one more edit. { $behind ->
    [0] It is the last one there is to walk
    [one] One more step is behind it
   *[other] { $behind } more steps are behind it
}
does-again = { $doing }. { $made-by }Redo — { $keys }. Each press puts back one more edit. { $behind ->
    [0] It is the last one there is to walk
    [one] One more step is behind it
   *[other] { $behind } more steps are behind it
}
no-cap = No cap
shown-of = { $shown } of { $count } tracks
