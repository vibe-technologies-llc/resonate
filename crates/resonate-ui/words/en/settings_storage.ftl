manage-storage = Manage storage
page-about = The files taking the most room, deleted or written smaller
largest-files = Largest files
largest-files-hint = Every file the library holds, the largest first, with its size and format. Delete moves a file to the trash and takes its song out of the library; while sync runs, Delete from every device asks your other devices to do the same. Re-compress writes a FLAC file again, smaller or at the quality chosen under Re-compressing, and keeps the original whenever the copy is not both verified and smaller.
largest-files-also-called = storage space size disk largest biggest files delete remove free room manage
recompressing = Re-compressing
recompressing-hint = Writes FLAC files again as tightly as the encoder can, or lowered to the highest quality chosen here. A copy replaces its file only once it has been decoded whole and read back as the sound it was made from, and only where it is smaller; otherwise the file is kept as it was. Tags and pictures are carried across, and a song keeps its plays, favourite and playlists.
recompressing-also-called = recompress compress re-encode shrink smaller resample downsample lower quality flac ceiling 96 48 cd
highest-quality-kept = Highest quality kept
as-it-is = Bit for bit
studio = 96 kHz
high = 48 kHz
cd = CD
as-it-is-means = The sound is kept exactly as it is; only the packing is made tighter, so nothing a listener could hear changes.
studio-means = A file above 96 kHz is resampled to the highest rate of its own family at or under it, and above 24 bits lowered to 24, with dither; anything at or under is kept bit for bit.
high-means = A file above 48 kHz is resampled to the highest rate of its own family at or under it, and above 24 bits lowered to 24, with dither; anything at or under is kept bit for bit.
cd-means = Every file is resampled to 44.1 kHz and lowered to 16 bits, with dither, as a CD holds it.
recompress-every-flac = Re-compress every FLAC
recompressing-every-flac = Re-compressing…
press-again-to-recompress = Press again to re-compress
ask-again-to-recompress = Press it again to write every FLAC file again. Each copy replaces its file only where it is verified and smaller; every other file is kept as it was.
stop = Stop
stopping = Stopping…
recompressing-so-far = { $weighed } of { $candidates } weighed · { $smaller } smaller · { $saved } saved
recompressing-note = A file cut by a cue sheet, kept in the vault or carrying a tag after its sound is left as it is.
reading-the-files = Reading the files…
no-files = The library holds no file yet.
songs-in-the-file = { $count ->
    [one] { $count } song
   *[other] { $count } songs
}
in-the-vault = kept in the vault
recompress-the-file = Re-compress { $name }
recompressing-the-file = Re-compressing { $name }…
delete-the-file = Delete { $name } from this device
delete-the-file-everywhere = Delete { $name } from every device
show-more = Show more
largest-files-note = Sizes are those the last scan read. A file a cue sheet cuts into songs is one entry, and deleting it takes every song it holds.
