add-a-folder = Add a music folder from Settings to scan one in.
add-songs = Start your library
add-songs-more = Choose the primary music folder: songs you drag onto the window are copied into it, and what is already there is scanned.
choose-primary-hint = Open the folder dialogue and make the folder chosen the primary music folder
drop-songs-more = Drag songs onto the window, or put them in the primary music folder, to fill the library.
library-unreadable = The library couldn't be read
library-unreadable-more = The log says what went wrong reading the catalog.
nothing-held-matches = Nothing in your library matches this search.
several = Various artists
forget-the-match = Not this record
other-pressings = Other pressings
other-pressings-hint = List every pressing of this record MusicBrainz holds, and take the one these files are
asking-for-pressings = Asking MusicBrainz for the pressings…
no-pressings = MusicBrainz named no pressings
find-the-record = Find the record
find-the-record-hint = Ask MusicBrainz for the releases named like this album, and take the one these files are
asking-for-releases = Asking MusicBrainz for releases named like this…
no-releases = MusicBrainz named no release like this
take-the-pressing-hint = Take this pressing for the album in place of the one in use
pressing-in-use-hint = The pressing the album is matched to now
forget-the-match-hint = The lookup took the wrong release: forget it, and never take it for this album again
forget-the-match-armed = Press again to forget the match
forget-the-match-armed-hint = Takes away the release, its rows and what it linked, and the next lookup asks again without it
order-hint = Choose what this listing is put in order by
play-all-hint = Play every track listed here in order, in place of the queue
shuffle-all-hint = Play every track listed here, shuffled
open-album-hint = See the tracks on this album
open-artist-hint = See every track by this artist
instead-hint = Search for the spelling the catalog holds instead
sung-hint = Search the lyrics for these words, in the order they were typed
want-hint = Mark this track wanted, so a provider can be asked for it
unwant-hint = Stop wanting this track
dismiss-missing-hint = Take this track off the Missing list, and stop wanting it if it was wanted
want-found-hint = Download this song: its release is added to the catalog and the providers are asked for it. Right-click to choose the release
fetch-found-hint = Download this song: its release is added to the catalog and the providers are asked for it now, the sidebar following how it goes
opening = Opening…
open-artist-found-hint = Open this artist to see their releases and download the songs you want
not-in-the-library = not in your library
open-album-not-held-hint = Open this album to see its songs and download the ones you want
get-album-rest-hint = Ask the providers for every missing track on this album
fetch-found-again-hint = Ask the providers for this song again
place-on = Place on a release…
look-it-up = Enrich this song
read-the-rest = Read the rest
read-the-rest-hint = Ask MusicBrainz for the releases past the first thousand this artist is credited on
nothing-places-it = Nothing has identified this track, so it is on no release yet
unheld-hint = See what this artist has released that the library does not hold
record-hint = About this record
genres-hint = Genres
favour-hint = Keep this among your favourites
unfavour-hint = Take this out of your favourites
add-songs-label = Add songs
no-albums-match = No albums match
no-albums-yet = No albums yet
albums = Albums
set-primary-folder = Set primary folder…
no-artists-match = No artists match
no-artists-yet = No artists yet
artists = Artists
no-tracks-yet = No tracks yet
no-tracks-match = No tracks match.
list = List
grid = Grid
all-tracks = All tracks
done = Done
save-this-search = Save this search
album = Album
artist = Artist
tracks = Tracks
in-use = In use
play = Play
shuffle = Shuffle
artists-list-hint = Draw the artists as a list, a small portrait beside each name
artists-grid-hint = Draw the artists as a grid of large portraits, the way albums are
find-the-releases-it-is-on = find the releases it is on
released = Released
format = Format
label = Label
catalogue-number = Catalogue number
barcode = Barcode
country = Country
kind = Kind
about-this-record = About this record
genres = Genres
on = On
all-lossless = all lossless
albums-counted = { $count ->
    [one] { $count } album
   *[other] { $count } albums
}
artists-counted = { $count ->
    [one] { $count } artist
   *[other] { $count } artists
}
tracks-counted = { $count ->
    [one] { $count } track
   *[other] { $count } tracks
}
plays-counted = { $count ->
    [one] { $count } play
   *[other] { $count } plays
}
did-you-mean = Did you mean { $instead }?
sung-in = Sung in { $count ->
    [one] { $count } track
   *[other] { $count } tracks
}
songs-not-held = Not in your library · { $count ->
    [one] { $count } song
   *[other] { $count } songs
} · press one to download it
add-songs-to = Add songs to { $playlist }
choose-tracks-to-add-to = Choose tracks to add to { $playlist }
unnamed-album = album { $id }
releases-not-held = { $count ->
    [one] { $count } release
   *[other] { $count } releases
} not held
releases-not-held-more-unread = { $count ->
    [one] { $count } release
   *[other] { $count } releases
} not held, { $unread } more unread
pressings = Pressings · { $count }
and-more = and { $count } more
undated = undated
pressing-tracks = { $count } tracks
releases-not-in-your-library = Not in your library · { $count ->
    [one] { $count } release
   *[other] { $count } releases
}
disc-named = Disc { $disc } · { $named }
disc = Disc { $disc }
discs = { $count } discs
on-services = On{"\u00A0"}
open-on = Open on { $service }
unnamed-artist = artist { $id }
lossless = { $count } lossless
shown = { $named } · { $drawn } shown
way-back-hint = Back to where this was opened from — { $keys }
