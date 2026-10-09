---
paths:
  - "crates/resonate-online/**/*.rs"
  - "crates/resonate/src/online.rs"
---

# The online crate

`resonate-online`: the one `Reference` for enrich, the one network `LyricProvider`, the one
`Corrections` for AutoEq, plus the printers, recognisers and `Scrobbler` behind their seams. Only
the binary depends on it, behind the `online` feature (`--exclude resonate-ui
--no-default-features`: no HTTP client; `cargo tree -p resonate-library`: no `ureq`/`serde`).

**Nothing about the listener leaves without their say.** `Config::online_enabled` (`online`, default
true) gates every request: `online::reference` is `None` when off, so `resonate enrich` says
`Error::OnlineOff` (build without the feature: `Error::NoReference`). `contact`, `acoustid-key`,
`audd-token`, `listenbrainz-token` and the `lastfm-` keys are empty by default, listener-set only.
No AcoustID key: no
fingerprint leaves. AudD: a clip only with a token. Shazam: no key, only a signature (peaks of what
was heard), when the listener asks to listen or `identify-by-sound` is on and a lookup has nothing
else to name a track by. ListenBrainz and Last.fm alone send something about the listener (every
counted play), only under a token or a session the listener signed in for.

## The client

- **One `Client` serves every host, saying only what this build is.** `Identity::user_agent_to` =
  `resonate/<version>`, plus ` ( <contact> )` only if `Identity::contact` is non-blank and the host
  `Host::asks_who_is_asking`: MetaBrainz (MusicBrainz, Cover Art Archive, ListenBrainz), Wikimedia
  (Commons, Wikidata). Others get bare name and version
  (`a_contact_is_told_to_the_hosts_that_ask_who_is_asking_and_no_other`). `online::identity` fills
  the contact from `Config::contact` alone. **A redirect carries the contact only to the host
  first asked**: ureq keeps every header but credentials across one, so a GET carrying a contact
  follows its redirects itself (`Client::got`, at most `REDIRECTS_FOLLOWED`), telling another host
  (the Cover Art Archive's archive.org) the bare name
  (`a_contact_is_not_carried_through_a_redirect_to_another_host`).
- **Identity and reachability are read per request.** `Introduction` = User-Agent behind a shared
  `RwLock`, written as the request's header: a contact typed in settings is heard by every sharing
  client from the next request (`online::INTRODUCTION`, `online::introduce`).
  `Introduction::following` carries a `Reintroduction` asked before each request; the binary's
  `Followed` reads only the `contact` key (`config::contact_in`), only where the config file's mtime
  moved, keeping what was said where the key will not read. `Client::reach` is a switch `exchange`
  reads before taking a turn: off, `Error::Offline` with nothing sent; `Setting::Online` stored in
  the window calls `online::reach`, so *Reach the network* off stops every service. The binary
  keeps the switch in `online::REACHING` too, set on the process client every time it is handed
  out, so a switch turned off before the client was first made still holds. Every seam's
  error reads `Offline` as `NetworkDown`.
- **Pacing reserves a slot (no sleep after a call), one queue per process.** `Client::pace`
  reserves from a `Pacing` (`BTreeMap<Host, Instant>` behind a `parking_lot::Mutex`): next slot =
  later of now and last slot + host interval, written back under the lock, slept towards outside
  it. Every `Client::new`/`Client::introduced` client shares
  `EVERY_CLIENT_IN_THE_PROCESS` (reference, `ByEar`, `AcoustId`, recognisers, any other `Online`
  take turns at MusicBrainz in one queue); only
  a test's `Client::on_clock` gets its own. The `*_INTERVAL` constants in `client.rs` are the
  per-host decisions (MusicBrainz, AcoustID: the rates they ask). Asserted on a `Faked` `Clock`'s
  record of sleeps.
- **The listener goes first; a yielding client waits.** A client is a `Listener` or, from
  `Client::yielding`, its `Yielding` twin (shares agent, introduction, queue, `reach` switch: off
  stops both). A listener's `exchange` holds a `Listening` guard on the host from before its first
  slot through every busy retry (`Turns::listening`); dropping stamps `listener_answered`, wakes the
  `Paced::answered` condvar. A yielding caller waits on it while any listener is counted, then asks
  only once the host is free (later of: last slot + interval, any `cooling`, listener's answer +
  interval), reserving nothing ahead: a listener waits at most the rest of one interval, never
  behind the pass. The post-answer interval lets a listener's next ask (`find_albums` after
  `find_songs`, an artist page's next group) take the next slot. Cost, meant: a lookup pass stands
  still while the listener keeps a host busy
  (`a_listener_asks_back_to_back_while_a_yielding_client_waits_for_it`,
  `a_yielding_client_waits_a_whole_interval_after_the_listeners_answer`). The switch is re-read
  after the wait (switched off while waiting: nothing sent).
  Binary: `online::Asking` says which a caller is; `client_for(config, InTheBackground)` is the one
  yielding twin of the process client; `reference`, `reference_asked_for`, `fingerprinters` take an
  `Asking`. A lookup pass gets the yielding pair wherever it starts (window's
  `Lookups::for_the_pass`, `resonate enrich`, the lookup after `resonate scan`, the MCP server), so
  its pictures and studies yield too; whatever a listener waits on (search, artist page, want,
  followed link, `share`, `analyse --recognise`) asks as the listener.
- **Busy: three more tries, wait doubling; `Retry-After` in seconds or an HTTP date, capped.**
  Busy = 503 (MusicBrainz's too-fast answer), 429, 502, 504; `BUSY_RETRIES`, `RETRY_AFTER_BY_DEFAULT` doubling,
  `RETRY_AFTER_AT_MOST` (a pass must not park for minutes a service names). The fourth answer is
  returned whatever it says. **A host busy through every retry is asked once, not four times, until
  it answers anything else** (`Pacing`'s `stayed_busy`; `retries_owed` answers none): an album's
  ladder of rungs costs one request each against a downed service, the library gets `Refused` at
  once. **A `Retry-After` on the returned answer is still waited on** (`cooling`, taken by
  `reserve`). `CONNECT_WITHIN`/`ANSWER_WITHIN` bound a request (unanswered: half a minute, not a
  run).
- **Status read, never raised; bodies capped.** The agent is `http_status_as_error(false)`: 404 =
  `Ok(None)` (release the service lacks, track LRCLIB has no words for), other failure =
  `Error::Refused { status }`. Body read via `Body::as_reader` one byte past the caller's cap
  (`LARGEST_DOCUMENT`, `LARGEST_PICTURE`, `LARGEST_INDEX`, `resonate_eq::LARGEST_PROFILE`); past it
  = `Error::TooLarge`. The cap is the caller's argument, weighed *after* the gzip decoder (ureq's
  `limit` wraps the socket beneath it; a thousand-to-one body would outgrow it:
  `a_cap_bounds_the_body_as_it_is_decoded_rather_than_as_it_arrived`). `Client::json` on a
  non-matching body: `Error::Unreadable` (reason: a debug record).
- **No plain HTTP.** `https_only` (redirect to plain = `Unreachable`); only tests' loopback servers
  are plain (`Carried::Plain`, compiled for them).
- **`ureq::Error` classified once, into four shapes** (`Error::from_ureq`): status = `Refused`;
  `Io`, timeout, unknown host, connect/proxy/TLS failure = `Unreachable`; `BodyExceedsLimit` =
  `TooLarge`; else `Unreadable`. `From<Error> for resonate_library::Error` drops the host: the
  library ends a pass on `Unreachable`, carries on past the rest; `TooLarge` is warned with its
  limit and stamped asked like a miss, counting toward no `REFUSALS_THAT_END_A_PASS` (asking again
  would not shrink the document). `into_lyric_error`, `into_eq_error`, `into_listen_error` map onto
  `resonate_lyrics::Error`, `resonate_eq::Error`, `resonate_listen::Error`.
- **Queries are escaped once, in `query.rs`** (`escape_query`, `lucene_quoted`, `Params`): an
  ampersand or quote in a title reaches a service as words, not syntax.

## MusicBrainz

Searches answer candidates; the strict taking rule lives in `enrich.rs` (one rule, one crate).
`Wording::Phrase`/`Wording::Words` = Lucene phrases / loose `dismax`; the
`a_*_query_names_only_what_was_asked` tests pin each byte.

- **A query names only what the ask carries.** A release query carries no track count or date (the
  library weighs both; naming them hides the pressing one track wider); artist by `arid:` where its
  id is known. A release group is searched under `releasegroup:`/`firstreleasedate:` where the
  release index spells `release`/`date` (an unknown Lucene field matches nothing rather than
  failing: a search silently empty for ever).
- **A release is asked whole in one request** (`RELEASE_INCLUDES`), mapped by
  `ReleaseDoc::into_release`. `stated` drops `[no label]`, `[none]`; a track's credit is kept as
  `ReleaseTrack::artist` only if it differs from the release's `credited_as`; non-mbid `id` =
  `Error::Unreadable`.
- **A recording is asked three ways through one document**: mbid; ISRC (`/isrc/<code>`;
  `ISRC_INCLUDES` omit release-groups, which that endpoint answers 400 to, so a release read there
  carries its status alone; one ISRC names every take released under it, telling them apart is the
  caller's); search. The search duration window `LENGTH_MAY_DIFFER_BY_MS` is wider than the five
  seconds `enrich.rs` accepts (service gives the neighbourhood, library decides inside). `codes`
  drops what `Isrc::new` will not hold.
- **The listener's words** (`find_songs`, the window's search for a song the catalog lacks) are
  capped at `SONGS_FOUND_AT_MOST` (the library discards every named recording and second take
  before offering the rest). **Words read as a title by an artist are asked as that first**:
  `songs_by_search` requires the artist (each word of `SPELT_LOOSELY_FROM` letters or more spelt
  loosely), the title only ranks; an empty answer falls back to the pair below. Title not
  required: MusicBrainz tokenises it as entered, so a phrase misses *You F O* under `"you fo"`.
  **Words read as no title by an artist are asked twice**: as the phrase an artist is credited
  under (`songs_credited_search`, answer leads) and as loose dismax words (dismax alone ranks a
  recording *titled* with the words, e.g. a cover or *Twenty One Pilots* by someone else, above the
  band's own songs: an artist-name search found none). **`find_albums` asks the same two ways of
  `/release-group/`** (`albums_credited_search`, then `albums_search`, each `ALBUMS_FOUND_AT_MOST`),
  reading `primary-type`, `secondary-types`, `first-release-date` into an `AlbumMatch`; the library
  decides which are albums.
- **A search answer is read for where each recording sits.** The index spells a medium's track list
  `track` (lookup: `tracks`, one `alias`) with no `position`, so `placed` falls back to
  `track-offset` + 1; `isrcs` pass the same filter, so a search-identified track learns its code
  with no second request.
- **A recording is placed, a release group counted; neither answer is the other's.**
  `/recording?inc=media` answers only the medium the recording is *on*; its `track-count` (disc 7 of
  a box set answers 6) is deliberately unread. `/release-group?inc=media` returns every medium of
  every release, so `counted` sums them into `GroupRelease::track_count`, the release's, which
  `closest_release` needs.
- **An artist's release groups are browsed, not searched; paged, capped.** `release_groups_of`
  browses narrowed to `DISCOGRAPHY_KINDS`, `BROWSE_PAGE` per page up to `GROUPS_AT_MOST`; the rest
  is `Discography::unread` (catalog keeps it, window says it); a `from` reads the next stretch.
  Secondary types (compilation, live) stay the library's `worth_keeping` alone; this crate keeps
  every group handed.
- **A group's songs come off its official pressings, one request** (`releases_of_group`, at most
  `PRESSINGS_READ`; unmappable pressing left out). The browse caps a page at 500 tracks, so a box
  set answers fewer pressings, never none. Which pressing speaks for the group: the library's
  (`songs::pressing_of`).
- **An artist's pressings: browsed a page at a time, every group at once** (`releases_of_artist`:
  `/release?artist=…&status=official&type=album|ep|single` with the pressings' includes, 100 asked
  per page). The service cuts a page with recordings at about 500 tracks, so a page holds what fits
  (Daughter's 58 releases in one, twenty one pilots' 173 in four, The Beatles' 2 217 in 28);
  `ArtistPressings` carries `credited` (`release-count`) and `read_to`, counted from the offset
  before unmappable releases are dropped, so the next page starts where this ended. One browse vs a
  request per group: the library's call (`learning.rs`).
- **A page is looked up for the artist MusicBrainz files it under** (`artist_at`:
  `/url?resource=<page>&inc=artist-rels`, page escaped whole). Unknown page = 404 = `None`; filed
  under two artists names neither (`LinkedUrlDoc::the_artist`). Which page spelling to ask: the
  library's (`ArtistLink::pages`).
- **Artist profile, genres, links: one request.** `genres` = tags with positive count, heaviest
  first (a single-editor tag does not outrank a hundred votes). `aliases` keep each spelling once by
  `folded` form, dropping any folding to the billed name. `links` carry every relation;
  `resonate_library::portrait_urls` walks the `Relation::Image` ones.

## Covers and portraits

**The front is asked at the size the window draws, in one request.** `coverart::cover` asks
`/release/<mbid>/front-500`, where that answers nothing the group's
`/release-group/<mbid>/front-500` (the archive redirects to the image: no index first). No 500
thumbnail yet = a miss until the next
ask. Bytes sniffed by `ImageFormat::sniff`: no picture = `Error::Unreadable`, not a `CoverArt` the
window cannot decode. Paced at `COVER_ARCHIVE_INTERVAL` (100 ms); the window fetches six found-song
covers at once, the lookup reads pictures on four threads.

`Reference::portrait` takes `&[Link]` (the library's retry hands it what the catalog holds), walking
until one answers: every `image` relation (Commons; all tried, the first may be a shop's
photograph), Wikidata, Wikipedia, Apple Music, Spotify, Deezer, SoundCloud. A link to any source
counts toward `may_be_pictured`, so a catalog enriched before a source joined is asked again by
`look_again_for_portraits`.

- **A refusal under 500 is a miss** (`passed_over_when_refused`): a taken-down page or a CDN not
  serving a region is a fact about that link. 429 is no miss (a host asking for fewer requests),
  leaving the artist asked again.
- **A failing source does not end the walk.** `Walk::tried` keeps the first other failure (429, 500
  and over, unreadable, too large, offline), asks the next source; only if nothing answers is it
  what the library is told (`Walk::ended`). `Unreachable` alone ends the walk.
- **Commons alone, scaled by the server.** `commons::named` reads `File:` page, `index.php?title=`,
  `Special:FilePath/`, `upload.wikimedia.org` shapes; `commons::scaled` writes
  `Special:FilePath/<name>?width=` at `PORTRAIT_WIDTH` (no original downloaded); other URLs: `None`.
  **A portrait in a format `ImageFormat::sniff` cannot read is a miss** (a cover's is
  `Error::Unreadable`): a file answered in its own format is a legitimate answer about that file,
  nothing reaches the artist's refusal count (`commons::drawable`). `commons::file_path` does not
  escape (a URL's name is already escaped).
- **Wikidata where no image relation is held.** Most artists have a `wikidata` relation and an
  entity with `P18`. The document is narrowed to `P18` by name so serde parses nothing else; the
  entities map is std's `HashMap` (keys come off a network). `wikidata::entity` refuses anything not
  `Q` + digits. **A claim's rank is weighed**: `preferred`, then `normal`, never `deprecated` (how
  Wikidata keeps a picture it calls wrong). The answered file name goes through
  `query::escape_query` into a path (about half have spaces).
- **A Wikipedia article leads to its entity** by `wbgetentities` on `Host::Wikidata` with the
  sitelink; `wikipedia::page` reads the language off the host (mobile `m.` passed over, `zh-yue`
  written `zh_yue`) and the title off `/wiki/`.
- **Apple Music: the artist's own picture off the page MusicBrainz links**, `og:image` read by
  `shared.rs`; `squared` asks mzstatic for a centred square crop. **A sleeve is no portrait**: with
  no artist picture Apple shows an album, so `squared` takes only a picture in an
  `AMCArtistImages…` or `Features…` bucket or named `pr_source`.
- **Spotify, SoundCloud: the artist's page on the service**, again `og:image`, taken only if an
  *artist* image (`i.scdn.co/image/ab676161…`, a sleeve being `ab67616d…`; an `avatars-…` file on
  `sndcdn.com`, the default avatar living elsewhere).
- **Deezer, by the link MusicBrainz holds**; public API, no key; no search by name (a held link
  names the artist, a name alone is a guess). **Its placeholder is no picture**: a hash segment of
  `NOTHING_HASHED` (MD5 of nothing) and the `DataException` document for an unknown number are
  misses. Deezer comes late: its picture CDN answers 403 to every request from some networks.
- Picture hosts checked by `shared::on_host`: https; host = what stands before the first `/`, `?`,
  `#` or backslash; letters, digits, `-`, `.` only.

## Links to a song, an album and a stream

- **`Reference::streamed_at` = `deezer::streamed`**, for a share the catalog holds no link for: by
  ISRC where there is one, else (also on an unknown ISRC's `DataException`) a plain-words search of
  artist + title (needs a non-blank artist; Deezer's field syntax answers an empty list for every
  query now). A hit needs title and artist folding to the asked ones through `folded_letters` and
  length within `LENGTHS_AGREE_WITHIN` where both known (edit or live takes passed over); only a
  `https://www.deezer.com/` link is answered.
- **`Reference::song_linked` = `linked::named_at`**: what a pasted link names (ISRCs, length, title,
  artist as `LinkNames`); weighing is `Library::follow_link` (`library.md`). A Deezer track is asked
  of Deezer's API. **Every other service is read through song.link's page, not its API** (which
  answers every keyless request `401 PUBLIC_API_ACCESS_DEPRECATED`): `page_data` cuts the
  `__NEXT_DATA__` JSON out of the page at `SongLink::page`; `Song::read` takes an `entityData` only
  of `type` `song`. The page names an ISRC for Spotify, TIDAL, SoundCloud, none for Apple Music or
  YouTube, so its `deezer|song|<n>` twin is asked whenever there is one, its code added after the
  page's. A page naming no code still names title and artist (how a YouTube upload is found); only
  one naming neither a code nor both names is nothing.
- **`Reference::album_linked` = `linked::album_named_at`**, answering `AlbumNames` (the `Barcode`s
  the album is sold under) via album.link's page under the same `Host::SongLink` pace;
  `releases_by_barcode` searches the release index's `barcode` under every leading-zero spelling
  (`Barcode::spellings`; MusicBrainz files a UPC-A as twelve digits or its thirteen-digit EAN). The
  Deezer twin's `upc` is added unless the same code a leading zero apart; both offered (Deezer may
  answer a reissue under another UPC). The library weighs the barcode, not the score
  (`Library::follow_album_link`); a MusicBrainz link asks neither.
- **`Reference::artist_linked` = the name Deezer's `/artist/<n>` bills**, from the `ArtistDoc` a
  portrait uses; a MusicBrainz artist link asks nothing, its id in hand
  (`an_artist_page_names_the_artist_it_is_of_and_an_empty_answer_names_nobody`).
- **`Reference::playlist_linked` reads a playlist's songs without a request a song.** Deezer:
  `/playlist/<n>` gives the title and the first tracks inline; `/playlist/<n>/tracks` pages of
  `PLAYLIST_SONGS_A_PAGE` follow until `nb_tracks` or `PLAYLIST_SONGS_AT_MOST` (1 000); each track
  is `TrackDoc::named` as a pasted track is. ListenBrainz: `/1/playlist/<mbid>`'s JSPF, each
  track's recording read off an `identifier` (one or a list) naming
  `musicbrainz.org/recording/<mbid>`. Spotify: the public `/embed/playlist/<id>` page, whose
  `__NEXT_DATA__` (cut by `shared::next_data`, song.link's reader too) holds the playlist's `name`
  and a `trackList` of title, `subtitle` (the artists as billed) and length in milliseconds; no
  account, no ISRC; the embed lists at most what Spotify puts on it (fifty to a hundred). Apple
  Music: the public `/<storefront>/playlist/<id>` page (`apple::playlist_named`), whose
  `serialized-server-data` script holds sections by `itemKind`: the `containerDetailHeaderLockup`
  item's `title` is the name, each `trackLockup` item a song (`title`, `artistName` as billed, else
  every `subtitleLinks` title joined, `duration` in milliseconds; a `contentDescriptor` kind other
  than `song`, a music video, passed over); no account, no ISRC
  (`a_link_to_an_apple_music_playlist_names_its_songs_off_the_public_page`, live). YouTube
  (`Host::Youtube`, the others' pace): the public `/playlist?list=<id>` page's `ytInitialData`
  (cut at the first `;</script>`), its name at `metadata.playlistMetadataRenderer.title`; the
  whole document is walked for entries rather than read down one path (`uploads_in`), since the
  page's layout moves: a `lockupViewModel` of `LOCKUP_CONTENT_TYPE_VIDEO` (title, the first
  metadata part as the channel, length off a `thumbnailBadgeViewModel` that reads as a clock) or
  the older `playlistVideoRenderer` (title runs, `shortBylineText`, `lengthSeconds`), each a
  `ListedSong::Uploaded`; the page holds the first hundred, the rest behind a continuation not
  asked (`a_link_to_a_youtube_playlist_names_its_uploads_off_the_public_page`, live). A playlist
  with no title names nothing.

## ListenBrainz

- **What was heard is posted as JSON under the listener's token, nothing else about them.**
  `ListenBrainz` is the `Scrobbler` handed to the library's `submit_listens`.
  `Posted::authorization` is the one header a body carries that others leave `None`; `listen_type` =
  `single` for one, `import` for several. A listen's moment is when it *began* (`listens.began`; the
  counted moment for a listen kept before the column); `media_player`, `submission_client` and its
  version name this build. `status` other than `ok` = `Unreadable`; an HTTP status = `Refused` as
  for every host (the library tells a malformed batch, 400, from a refused token, 401, by it).
  **What is playing is told as it starts**: `Scrobbler::playing_now` posts a `playing_now` listen
  with no `listened_at`, shown by the service, never counted.
- **A favourite is a love; a token is asked who holds it.** `Scrobbler::love` posts `score` 1, or 0
  for `Love::TakenBack` (no feedback, not a hate); a track with no recording MBID is never told.
  `Library::tell_loves` weighs favourites against `loves_told` per `ListeningService`, telling the
  difference either way, at most `LOVES_TOLD_AT_ONCE` per call (the submitter's look is not held a
  minute behind a first run). A love refused with any 4xx but 401, 403, 408, 429 (the service saying
  this recording will never be taken) is noted told so it cannot block the rest; any other failure
  returns at once, the love asked again. A favourite is a state: every one held when a token is
  first given is told. `Scrobbler::token_held` asks `validate-token` via `Client::json_as` (the one
  GET carrying an `Authorization`), answering `TokenHeld::By` the user, or `Unknown` (also for 400,
  401, 403).
- **The binary's half is a thread following the file.** `submitting.rs` starts `resonate-submit` for
  the window and every playing command: after `FIRST_AFTER`, then every `SUBMITTED_EVERY`, it asks
  `Library::submit_listens`, doubling the wait per failure up to `WAITED_AT_MOST`; loves have their
  own `Pace` (a failing love backs off alone). Each service the settings hold an account for is a
  `Telling` of its own (paces, refused account, what it was told plays), so a refused or failing
  Last.fm session holds back nothing told to ListenBrainz. Every `PLAYING_LOOKED_AT_EVERY` it looks at the
  player and tells a new row via `Library::billed_as` as playing now: once per row, not while
  paused, never for a file the catalog names nothing for; `lapses` tells again after a resume, a
  repeat coming round or a seek back (the service's *playing now* does not run out). The accounts
  follow the file as `Followed` does (`config::submitting_in` reads `online`,
  `listenbrainz-token` and the three `lastfm-` keys into `Accounts`); a client is built once per
  account (connections and pacing
  outlive the looks). A 401 or 403 to listens or loves (`TOKEN_REFUSED`) holds that token back until
  the file names another.

## Last.fm

- **What was heard is scrobbled under a session the listener signed in for, signed by an
  application the listener registered.** `LastFm` is the second `Scrobbler`
  (`ListeningService::LastFm`, `lastfm`: the catalog's `submissions` and `loves_told` rows are
  per service, so neither service's mark moves the other's). Every call is a form POST to
  `Host::LastFm` signed as the API asks: `api_sig` = MD5 of every field but `format`, sorted by
  name, written name then value, then the application's secret (`Fields::signature`), `format=json`
  after. `track.scrobble` in batches of `SCROBBLED_AT_ONCE` (50), each listen numbered (`artist[i]`,
  `track[i]`, `timestamp[i]` when it began, album, number, recording, length);
  `track.updateNowPlaying` for what plays; `user.getInfo` answers who holds the session. An error
  in the answer is read by its code (`refused`): an invalid session, key or authentication is
  `Refused` 401 (the submitter holds that session back, as a refused ListenBrainz token), a rate
  limit 429, the service down 503, the rest unreadable. **A favourite is loved there by its
  names**: loves are owed by recording (`loves_told` per service), and `Library::tell_loves` hands
  each `Scrobbler::love` a `Loved`: the recording and the `LovedNames` (title, artist) of the
  catalog track carrying it, a favourite first (`THE_NAMES_OF_A_RECORDING`). Last.fm signs
  `track.love` or `track.unlove` with `artist` and `track`; a recording no track names any more
  is told nothing there and marked told. ListenBrainz reads the recording alone. Before this a
  Last.fm love answered done unsent, so a migration step forgets every `loves_told` row of
  `lastfm` and the next pass sends them.
- **A session is asked once, by name and password, and the password is not kept.**
  `resonate_online::signed_in` asks `auth.getMobileSession`; `resonate lastfm --user` reads the
  password from standard input (echo off on a terminal, `input::a_line_unechoed`) and stores only
  the session key (`lastfm-session`); `--forget` clears it. The window asks the same call through
  `Scrobblers::signed_in_to_lastfm` (a `LastfmSignIn` in, a `LastfmSession` out, neither printing
  a secret in its `Debug`), which the binary's `ByToken` answers with `lastfm_signed_in`.
  `lastfm::Application` and `lastfm::Session` likewise print the key and name alone, and the
  window's `settings::Online` (so `Supplying`) prints `<withheld>` for every token, password,
  secret and the contact (`an_application_and_a_session_print_neither_secret_nor_session_key`,
  `the_online_settings_print_no_secret_and_no_contact`).

## LRCLIB

- **`Lrclib` reads what the catalog kept before asking, and keeps what it is told.** It refuses a
  `Wanted` with no title or artist. `Library::kept_lyrics` is read first, keyed by the `Wanted`'s
  location and `Wanted::span`: a kept row not `KeptLyrics::is_due` answers without a request (its
  words, or nothing for a kept miss); a due one is asked again, the richer of kept and answer going
  to the pane, a failed request falling back to the kept. No library: keeps nothing.
- **Window and lookup ask one way**: `lrclib::told` is the whole question, mapped by `Lrclib` into a
  lyric error and by `Reference::lyrics` into the library's.
- **`/get` is exact, `/search` weighed.** 404 on `/get` falls through to `/search`; `pick` takes
  answers that `names` the track (title and artist folded to lowercase alphanumerics) and
  `lasts_about` as long within `LENGTH_MAY_DIFFER_BY`, best by `Worded` (synced, plain, wordless,
  instrumental), then nearest length. An `instrumental` or wordless answer is `None`, kept as a
  miss. `LyricText` = `syncedLyrics` else `plainLyrics` (else the lyricsfile's lines), with the
  answer's `lyricsfile` beside it **only where that document says more than its lines** (LRCLIB
  writes a Lyricsfile for every record; one written from an LRC times no word and is the LRC again
  at four times the size).
- **A Lyricsfile is read by `resonate-lyrics`** (`lyrics.md`: reader and sidecar side); LRCLIB's
  answer goes through the same `read_lyricsfile`, a kept document is read back through it, one no
  longer reading falls back to the kept LRC text.

## Recognising a clip

- **Two services behind `resonate-listen`'s `Recogniser`, asked in order.** `online::recognisers`
  registers `Shazam` wherever `online` is on and `Audd` where `audd-token` holds one (not AcoustID,
  which cannot match a clip: `analysis.md`). `Recognisers::recognise` takes the first that names the
  song; a silent clip goes nowhere (`Error::NothingHeard`); a refusal is a `tracing` record and a
  name in `Recognition::refused`, not the end of the walk.
- **`Client::exchange` is the one loop for GET and POST**, `Posted` body or none. A POST carries
  `Content-Language: en_US`, and `Content-Encoding: gzip` only where `Posted::packed_form` made it.
- **Shazam is asked a signature, never audio.** `Shazam::recognise` posts `signature_of` the clip's
  mono mix with fresh `uuid` v4 ids, a zeroed location, UTC and this build's agent. Empty `matches`
  = nothing heard. The cover (`coverarthq`) comes from `AppleArtwork` only over https on
  `mzstatic.com` or a subdomain (`shared::on_host`); the song's page only over https on
  `shazam.com`, else `Heard::link` is `None`, so *Open* hands the desktop nothing else. The endpoint is undocumented, likeliest to
  change; `shazam_answers_a_clip_it_does_not_know_with_nothing_rather_than_a_refusal` is the live
  test that notices.
- **AudD gets the clip itself, only with a token the listener typed**: mono mix as 16-bit WAVE in
  `multipart/form-data`, refusing a clip past `LARGEST_CLIP`. `status` other than `success` =
  `Refused` with the service's error code; `null` result = nothing heard. `song_link` is kept only
  over https on `lis.tn`, AudD's own page.
- **A file nothing else can name is named by ear, where the listener said so.** `ByEar` is a
  `Fingerprints` registered after `AcoustId` wherever `online` is on, answering only while the
  shared `identify-by-sound` switch is on (`Fingerprints::answers`, weighed by
  `Fingerprinters::has_a_source` and `recognise`: a switch off costs no study). It signs a
  twelve-second `resonate_analysis::excerpt` with `Shazam::signed`, then turns what Shazam names
  into recordings: its ISRC via `recordings_of_isrc`, takes within its own `LENGTHS_AGREE_WITHIN` of
  the file's length scored whole, else a `find_recording` phrase search scored as MusicBrainz scored
  it. The pass weighs them as any recognition (`Certainty::Nearly`: a name the file never gave is
  filled, none it gave touched), naming a file with no title, no artist and a stem saying nothing.

## Fixtures and the live test

**Mapping is proved over captured answers; services are reached only on request.**
`tests/fixtures/` holds a captured document per shape, pulled into each module's tests by
`include_str!`, so document-to-model runs on every `cargo test` with no network. Captured whole, not
cut down; the AudD ones were written from the service's documentation. `tests/live.rs` is gated on
`RESONATE_ONLINE_TESTS` and shares one client so pacing holds across its tests.

## The binary's half

**`crates/resonate/src/online.rs` is the only file naming `resonate_online`'s types, and has a twin
for the build without it.** Under the feature: `reference` answers an `Online` only where
`Config::online_enabled`; `corrections` = `Corrected::uncorrected()` + an `AutoEq` under the same
condition; `lyricists` = `Lyricists::local_choosing_by_the_locale` + an `Lrclib` (the last two get
the library where there is one, so the catalog keeps what each fetched); `fingerprinters` and
`recognisers` register the printers and recognisers above. `reference_asked_for` and
`corrections_asked_for` = the same two or `Error::OnlineOff` (the second asks
`Corrected::has_a_source`, not the key again). **An AutoEq answer is kept only once it reads**:
an index naming no device, or a profile `read_profile` makes no correction of (a captive portal's
page), is `Unreadable`, never cached for the index's 30 days or a device's 90. Without the feature the twins answer `None`,
`Error::NoReference`, `Corrected::uncorrected()` and the stubs, so `main.rs` reads the same names
either way. All is built over the one `Arc<Client>` in `online::CLIENT` (made on first use over
`INTRODUCTION`; `online::YIELDING` holds its yielding twin): one agent and its connections per
process.
