---
paths:
  - "crates/resonate-online/**/*.rs"
  - "crates/resonate/src/online.rs"
---

# The online crate

`resonate-online`: the one `Reference` for enrich, the one network `LyricProvider`, the one
`Corrections` for AutoEq, plus the printers, recognisers and `Scrobbler` behind their seams. The binary composes it behind the `online` feature, and the mobile boundary composes it for Android and iOS
behind its persisted runtime switch, enabled by default when unset. `cargo tree -p resonate --no-default-features` has no HTTP
client; `cargo tree -p resonate-library` has no `ureq`/`serde`. Android capture modules in
`resonate-listen` are excluded while its recognizer vocabulary remains available.

**Nothing about the listener leaves without their say.** `Config::online_enabled` (`online`, default
true) gates every request: `online::reference` is `None` when off, so `resonate enrich` says
`Error::OnlineOff` (build without the feature: `Error::NoReference`). `contact`, `acoustid-key`,
`audd-token`, `listenbrainz-token` and the `lastfm-` keys are empty by default, listener-set only.
No AcoustID key: no fingerprint leaves. AudD: a clip only with a token. Shazam: no key, only a
signature, when the listener asks to listen or `identify-by-sound` is on and a lookup has nothing
else to name a track by. ListenBrainz and Last.fm alone send something about the listener (every
counted play), only under a token or session the listener signed in for.

## The client

- **One `Client` serves every host, saying only what this build is.** `Identity::user_agent_to` =
  `resonate/<version>`, plus ` ( <contact> )` only if `Identity::contact` is non-blank and the host
  `Host::asks_who_is_asking`: MetaBrainz (MusicBrainz, Cover Art Archive, ListenBrainz), Wikimedia
  (Commons, Wikidata) (`a_contact_is_told_to_the_hosts_that_ask_who_is_asking_and_no_other`).
  `online::identity` fills the contact from `Config::contact` alone. **A redirect carries the contact
  only to the host first asked**: ureq keeps every header but credentials across one, so a GET
  carrying a contact follows redirects itself (`Client::got`, at most `REDIRECTS_FOLLOWED`)
  (`a_contact_is_not_carried_through_a_redirect_to_another_host`).
- **Identity and reachability are read per request.** `Introduction` = User-Agent behind a shared
  `RwLock` (`online::INTRODUCTION`, `online::introduce`): a contact typed in settings is heard by
  every sharing client from the next request. `Introduction::following` carries a `Reintroduction`
  asked before each request; the binary's `Followed` reads only the `contact` key
  (`config::contact_in`), only where the config file's mtime moved. `Client::reach` is a switch
  `exchange` reads before taking a turn: off, `Error::Offline` with nothing sent
  (`Setting::Online` calls `online::reach`). The binary also keeps it in `online::REACHING`, set on
  the process client every time it is handed out, so a switch turned off before the client was first
  made still holds. Every seam's error reads `Offline` as `NetworkDown`.
- **Pacing reserves a slot (no sleep after a call), one queue per process.** `Client::pace`
  reserves from a `Pacing` (`BTreeMap<Host, Instant>` behind a `parking_lot::Mutex`): next slot =
  later of now and last slot + host interval, written back under the lock, slept towards outside
  it. Every `Client::new`/`Client::introduced` client shares `EVERY_CLIENT_IN_THE_PROCESS`; only a
  test's `Client::on_clock` gets its own (asserted on a `Faked` `Clock`'s record of sleeps). The
  `*_INTERVAL` constants in `client.rs` are the per-host decisions.
- **The listener goes first; a yielding client waits.** A client is a `Listener` or, from
  `Client::yielding`, its `Yielding` twin (shares agent, introduction, queue, `reach` switch). A
  listener's `exchange` holds a `Listening` guard on the host from before its first slot through
  every busy retry (`Turns::listening`); dropping stamps `listener_answered` and wakes the
  `Paced::answered` condvar. A yielding caller waits while any listener is counted, then asks only
  once the host is free (later of: last slot + interval, any `cooling`, listener's answer +
  interval), reserving nothing ahead: a listener waits at most the rest of one interval, never
  behind the pass. The cost, meant: a lookup pass stands still while the listener keeps a host busy
  (`a_listener_asks_back_to_back_while_a_yielding_client_waits_for_it`,
  `a_yielding_client_waits_a_whole_interval_after_the_listeners_answer`). The switch is re-read
  after the wait.
  Binary: `online::Asking` says which a caller is; `client_for(config, InTheBackground)` is the one
  yielding twin of the process client; `reference`, `reference_asked_for`, `fingerprinters` take an
  `Asking`. A lookup pass gets the yielding pair wherever it starts (window's
  `Lookups::for_the_pass`, `resonate enrich`, the lookup after `resonate scan`, the MCP server);
  whatever a listener waits on (search, artist page, want, followed link, `share`, `analyse
  --recognise`) asks as the listener.
- **Busy: three more tries, wait doubling; `Retry-After` in seconds or an HTTP date, capped.**
  Busy = 503, 429, 502, 504; `BUSY_RETRIES`, `RETRY_AFTER_BY_DEFAULT`, `RETRY_AFTER_AT_MOST`. **The
  ladder is the host's, not the thread's**: `Pacing::backed_off` records each wait as the host's
  `cooling` and climbs one doubling step per host, so threads hearing the same busy answer wait out
  the same wait and the next one starts higher; a non-busy answer resets it
  (`threads_hearing_the_same_busy_answer_climb_one_ladder_between_them`). The
  fourth answer is returned whatever it says. **A host busy through every retry is asked once, not
  four times, until it answers anything else** (`Pacing`'s `stayed_busy`; `retries_owed`): the
  library gets `Refused` at once. **A `Retry-After` on the returned answer is still waited on**
  (`cooling`, taken by `reserve`). `CONNECT_WITHIN`/`ANSWER_WITHIN` bound a request.
- **Status read, never raised; bodies capped.** The agent is `http_status_as_error(false)`: 404 =
  `Ok(None)`, other failure = `Error::Refused { status }`. The body is read via `Body::as_reader`
  one byte past the caller's cap (`LARGEST_DOCUMENT`, `LARGEST_PICTURE`, `LARGEST_INDEX`,
  `resonate_eq::LARGEST_PROFILE`); past it = `Error::TooLarge`. The cap is weighed *after* the gzip
  decoder (ureq's `limit` wraps the socket beneath it:
  `a_cap_bounds_the_body_as_it_is_decoded_rather_than_as_it_arrived`). `Client::json` on a
  non-matching body: `Error::Unreadable`.
- **No plain HTTP.** `https_only` (redirect to plain = `Unreachable`); only tests' loopback servers
  are plain (`Carried::Plain`).
- **`ureq::Error` classified once, into four shapes** (`Error::from_ureq`): status = `Refused`;
  `Io`, timeout, unknown host, connect/proxy/TLS failure = `Unreachable`; `BodyExceedsLimit` =
  `TooLarge`; else `Unreadable`. `From<Error> for resonate_library::Error` drops the host: the
  library ends a pass on `Unreachable`; `TooLarge` is warned, stamped asked like a miss, and counts
  toward no `REFUSALS_THAT_END_A_PASS` (asking again would not shrink the document).
  `into_lyric_error`, `into_eq_error`, `into_listen_error` map onto the lyrics, eq and listen errors;
  a status stays a `Refused { provider, op, status }` on the lyric and eq seams (a 429 from LRCLIB or
  GitHub can be backed off from), only `Unreadable` and `TooLarge` read as `Unreadable`
  (`the_library_and_the_lyric_crate_each_read_the_error_in_their_own_vocabulary`).
- **Queries are escaped once, in `query.rs`** (`escape_query`, `lucene_quoted`, `Params`): an
  ampersand or quote in a title reaches a service as words, not syntax.

## MusicBrainz

Searches answer candidates; the strict taking rule lives in `enrich.rs`. `Wording::Phrase`/
`Wording::Words` = Lucene phrases / loose `dismax`; the `a_*_query_names_only_what_was_asked` tests
pin each byte.

- **A query names only what the ask carries.** A release query carries no track count or date (the
  library weighs both; naming them hides the pressing one track wider); artist by `arid:` where its
  id is known. A release group is searched under `releasegroup:`/`firstreleasedate:` where the
  release index spells `release`/`date` (an unknown Lucene field matches nothing rather than
  failing).
- **A release is asked whole in one request** (`RELEASE_INCLUDES`), mapped by
  `ReleaseDoc::into_release`. `stated` drops `[no label]`, `[none]`; a track's credit is kept as
  `ReleaseTrack::artist` only if it differs from the release's `credited_as`; non-mbid `id` =
  `Error::Unreadable`.
- **A recording is asked three ways through one document**: mbid; ISRC (`/isrc/<code>`;
  `ISRC_INCLUDES` omit release-groups, which that endpoint answers 400 to; one ISRC names every take
  released under it, telling them apart is the caller's); search. **A release's track keeps every
  code its recording is registered under** (`ReleaseTrack::isrcs`, through `codes`, which drops what
  `Isrc::new` will not hold). The search duration window `LENGTH_MAY_DIFFER_BY_MS` is wider than the
  five seconds `enrich.rs` accepts (service gives the neighbourhood, library decides inside).
- **The listener's words** (`find_songs`, the window's search for a song the catalog lacks) are
  asked `SONGS_FOUND_AT_MOST` (100) at a time, since the library ranks what comes back by the
  releases each recording is on and a title many artists recorded leads with covers in any smaller
  slice (*creep* and *bohemian rapsody* answered 25 at a time left Radiohead and Queen out; a hundred
  holds them, their takes summing to 191 and 460 releases). A hundred recordings with their release
  lists run to a megabyte or more, so `songs_found` reads under `LARGEST_SONG_SEARCH` (16 MiB), not
  `LARGEST_DOCUMENT`. **A query names no field of a release**: `status:`, `primarytype:`, `date:` and
  the like narrow the `releases` listed on each recording to the releases matching them (adding
  `status:official primarytype:album` cut Radiohead's *Creep* from 191 releases to 7), which is the
  signal the ranking reads. **A hundred of hundreds of equal scores is a different hundred each
  time** (*creep* matches 719 recordings naming an ISRC; the same query answered without Radiohead's
  and with it), so the plain-words coded search adds an optional `tag:*`, a field of the recording:
  the recordings people have tagged are the ones they know, and with it Radiohead's *Creep* is in
  the answer each time, followed by Nirvana's and TLC's (`a_title_many_artists_recorded_and_a_typo_of_one_find_the_original_first`,
  live). `find_songs` is `found_for` over an `ask` closure, tested offline over
  canned answers. Words are split by `resonate_core::words_of_a_name`, so *don't* is sent as
  *dont*. The order of asking:
  1. **Words read as a title by an artist are asked as that first, and taken only where an answer
     bears the reading out** (`songs_by_search`): the artist is required (each word of
     `SPELT_LOOSELY_FROM` letters or more spelt loosely), the title only ranks. Asked first among
     recordings naming an ISRC (`+isrc:*`, `Coded::Required`: MusicBrainz scores every recording of
     a title alike, so uncoded answers lead with compilations and bootlegs), then without; taken
     only where one recording is `ByArtist::answered_by` (credits the typed artist exactly or
     `spelt_alike`, a leading *the* aside, and names the typed title). Else the plain way below, and
     what the reading's artist sang is kept after that answer (*stand by me* is no song *stand* by
     an artist *me*).
  2. **Plain words are asked as an artist's name and among coded recordings**
     (`songs_credited_search`, then `songs_coded_search`), the coded answer kept where one recording
     holds every word in its title or credit (`answers_every_word`, `Spelt::AsTyped`); **else asked
     again with words of `SPELT_LOOSELY_FROM` letters or more spelt loosely** (`Spelt::Loosely`,
     `w~`); else the loose dismax words (`songs_search`).
  The order of the answer is not its rank: the library ranks. **`find_albums` asks the same two ways
  of `/release-group/`** (`albums_credited_search`, then `albums_search`, each
  `ALBUMS_FOUND_AT_MOST`), reading `primary-type`, `secondary-types`, `first-release-date` into an
  `AlbumMatch`; the library decides which are albums.
- **A search answer is read for where each recording sits.** The index spells a medium's track list
  `track` (lookup: `tracks`) with no `position`, so `placed` falls back to `track-offset` + 1;
  `isrcs` pass the same filter, so a search-identified track learns its code with no second request.
- **A recording is placed, a release group counted; neither answer is the other's.**
  `/recording?inc=media` answers only the medium the recording is *on*; its `track-count` is
  deliberately unread. `/release-group?inc=media` returns every medium of every release, so
  `counted` sums them into `GroupRelease::track_count`, which `closest_release` needs.
- **An artist's release groups are browsed, not searched; paged, capped.** `release_groups_of`
  browses narrowed to `DISCOGRAPHY_KINDS`, `BROWSE_PAGE` per page up to `GROUPS_AT_MOST`; the rest
  is `Discography::unread` (a `from` reads the next stretch). Secondary types stay the library's
  `worth_keeping` alone; this crate keeps every group handed.
- **A group's songs come off its official pressings, one request** (`releases_of_group`, at most
  `PRESSINGS_READ`; unmappable pressing left out). The browse caps a page at 500 tracks, so a box
  set answers fewer pressings, never none. Which pressing speaks for the group: the library's
  (`songs::pressing_of`).
- **An artist's pressings: browsed a page at a time, every group at once** (`releases_of_artist`:
  `/release?artist=…&status=official&type=album|ep|single`, 100 asked per page). The service cuts a
  page with recordings at about 500 tracks, so a page holds what fits (The Beatles' 2 217 releases
  in 28); `ArtistPressings` carries `credited` (`release-count`) and `read_to`, counted from the
  offset before unmappable releases are dropped, so the next page starts where this ended.
- **A page is looked up for the artist MusicBrainz files it under** (`artist_at`:
  `/url?resource=<page>&inc=artist-rels`, page escaped whole). Unknown page = 404 = `None`; filed
  under two artists names neither (`LinkedUrlDoc::the_artist`). Which page spelling to ask: the
  library's (`ArtistLink::pages`).
- **Artist profile, genres, links: one request.** `genres` = tags with positive count, heaviest
  first. `aliases` keep each spelling once by `folded` form, dropping any folding to the billed
  name. `links` carry every relation; `resonate_library::portrait_urls` walks the `Relation::Image`
  ones.

## Covers and portraits

**The front is asked at the size the window draws, in one request.** `coverart::cover` asks
`/release/<mbid>/front-500`, where that answers nothing the group's
`/release-group/<mbid>/front-500` (the archive redirects to the image: no index first). No 500
thumbnail yet = a miss until the next ask. Bytes sniffed by `ImageFormat::sniff`: no picture =
`Error::Unreadable`. Paced at `COVER_ARCHIVE_INTERVAL` (100 ms).

`Reference::portrait` takes `&[Link]`, walking until one answers: every `image` relation (Commons;
all tried, the first may be a shop's photograph), Wikidata, Wikipedia, Apple Music, Spotify,
Deezer, SoundCloud. A link to any source counts toward `may_be_pictured`, so a catalog enriched
before a source joined is asked again by `look_again_for_portraits`.

- **A refusal under 500 is a miss** (`passed_over_when_refused`): a taken-down page or a CDN not
  serving a region is a fact about that link. 429 is no miss, leaving the artist asked again.
- **A failing source does not end the walk.** `Walk::tried` keeps the first other failure, asks the
  next source; only if nothing answers is it what the library is told (`Walk::ended`).
  `Unreachable` alone ends the walk.
- **Commons alone, scaled by the server.** `commons::named` reads `File:` page, `index.php?title=`,
  `Special:FilePath/`, `upload.wikimedia.org` shapes; `commons::scaled` writes
  `Special:FilePath/<name>?width=` at `PORTRAIT_WIDTH`; other URLs: `None`. **A portrait in a format
  `ImageFormat::sniff` cannot read is a miss** and counts toward nothing in the artist's refusal
  count (`commons::drawable`). `commons::file_path` does not escape (a URL's name is already
  escaped).
- **Wikidata where no image relation is held.** The document is narrowed to `P18` by name; the
  entities map is std's `HashMap` (keys come off a network). `wikidata::entity` refuses anything not
  `Q` + digits. **A claim's rank is weighed**: `preferred`, then `normal`, never `deprecated`. The
  answered file name goes through `query::escape_query` into a path.
- **A Wikipedia article leads to its entity** by `wbgetentities` on `Host::Wikidata` with the
  sitelink; `wikipedia::page` reads the language off the host (mobile `m.` passed over, `zh-yue`
  written `zh_yue`) and the title off `/wiki/`.
- **Apple Music: the artist's own picture off the page MusicBrainz links**, `og:image` read by
  `shared.rs`; `squared` asks mzstatic for a centred square crop. **A sleeve is no portrait**:
  `squared` takes only a picture in an `AMCArtistImages…` or `Features…` bucket or named
  `pr_source`.
- **Spotify, SoundCloud: the artist's page on the service**, `og:image`, taken only if an *artist*
  image (`i.scdn.co/image/ab676161…`, a sleeve being `ab67616d…`; an `avatars-…` file on
  `sndcdn.com`).
- **Deezer, by the link MusicBrainz holds**; public API, no key; no search by name. **Its
  placeholder is no picture**: a hash segment of `NOTHING_HASHED` and the `DataException` document
  for an unknown number are misses. Deezer comes late: its picture CDN answers 403 on some networks.
- Picture hosts checked by `shared::on_host`: https; host = what stands before the first `/`, `?`,
  `#` or backslash; letters, digits, `-`, `.` only.

## Links to a song, an album and a stream

- **`Reference::streamed_at` = `deezer::streamed`**, for a share the catalog holds no link for: by
  ISRC where there is one, else (also on an unknown ISRC's `DataException`) a plain-words search of
  artist + title (needs a non-blank artist). A hit needs title and artist folding to the asked ones
  through `folded_letters` and length within `LENGTHS_AGREE_WITHIN` where both known; only a
  `https://www.deezer.com/` link is answered.
- **`Reference::song_linked` = `linked::named_at`**: what a pasted link names (ISRCs, length, title,
  artist as `LinkNames`); weighing is `Library::follow_link` (`library.md`). A Deezer track is asked
  of Deezer's API. **Every other service is read through song.link's page, not its API** (which
  answers keyless requests `401 PUBLIC_API_ACCESS_DEPRECATED`): `page_data` cuts the `__NEXT_DATA__`
  JSON out of the page at `SongLink::page`; `Song::read` takes an `entityData` only of `type`
  `song`. The page names no ISRC for Apple Music or YouTube, so its `deezer|song|<n>` twin is asked
  whenever there is one, its code added after the page's. A page naming no code still names title
  and artist (how a YouTube upload is found); only one naming neither a code nor both names is
  nothing.
- **`Reference::album_linked` = `linked::album_named_at`**, answering `AlbumNames` (the `Barcode`s)
  via album.link's page under the same `Host::SongLink` pace; `releases_by_barcode` searches the
  release index's `barcode` under every leading-zero spelling (`Barcode::spellings`; UPC-A as twelve
  digits or EAN as thirteen). The Deezer twin's `upc` is added unless the same code a leading zero
  apart. The library weighs the barcode, not the score (`Library::follow_album_link`); a MusicBrainz
  link asks neither.
- **`Reference::artist_linked` = the name Deezer's `/artist/<n>` bills**, from the `ArtistDoc` a
  portrait uses; a MusicBrainz artist link asks nothing, its id in hand
  (`an_artist_page_names_the_artist_it_is_of_and_an_empty_answer_names_nobody`).
- **`Reference::playlist_linked` reads a playlist's songs without a request a song.**
  - Deezer: `/playlist/<n>` gives the title and first tracks inline; `/playlist/<n>/tracks` pages of
    `PLAYLIST_SONGS_A_PAGE` follow until `nb_tracks` or `PLAYLIST_SONGS_AT_MOST` (1 000).
  - ListenBrainz: `/1/playlist/<mbid>`'s JSPF, each recording read off an `identifier` naming
    `musicbrainz.org/recording/<mbid>`.
  - Spotify: the public `/embed/playlist/<id>` page, whose `__NEXT_DATA__` (`shared::next_data`)
    holds `name` and a `trackList` of title, `subtitle` (artists as billed) and milliseconds; no
    ISRC; at most what Spotify puts on the embed (fifty to a hundred).
  - Apple Music: the public `/<storefront>/playlist/<id>` page (`apple::playlist_named`), whose
    `serialized-server-data` script holds sections by `itemKind`: `containerDetailHeaderLockup`
    `title` is the name, each `trackLockup` a song (a `contentDescriptor` kind other than `song`
    passed over); no ISRC (`a_link_to_an_apple_music_playlist_names_its_songs_off_the_public_page`,
    live).
  - YouTube (`Host::Youtube`, the others' pace): the public `/playlist?list=<id>` page's
    `ytInitialData`, name at `metadata.playlistMetadataRenderer.title`; the whole document is walked
    for entries (`uploads_in`) since the layout moves: a `lockupViewModel` of
    `LOCKUP_CONTENT_TYPE_VIDEO` or the older `playlistVideoRenderer`, each a `ListedSong::Uploaded`;
    the page holds the first hundred, the continuation is not asked
    (`a_link_to_a_youtube_playlist_names_its_uploads_off_the_public_page`, live).
  A playlist with no title names nothing.

## ListenBrainz

- **What was heard is posted as JSON under the listener's token, nothing else about them.**
  `ListenBrainz` is the `Scrobbler` handed to the library's `submit_listens`.
  `Posted::authorization` is the one header a body carries that others leave `None`; `listen_type` =
  `single` for one, `import` for several. A listen's moment is when it *began* (`listens.began`);
  `media_player`, `submission_client` and its version name this build. `status` other than `ok` =
  `Unreadable`; an HTTP status = `Refused` (the library tells a malformed batch, 400, from a refused
  token, 401, by it). **What is playing is told as it starts**: `Scrobbler::playing_now` posts a
  `playing_now` listen with no `listened_at`, shown by the service, never counted.
- **A favourite is a love; a token is asked who holds it.** `Scrobbler::love` posts `score` 1, or 0
  for `Love::TakenBack`; a track with no recording MBID is never told. `Library::tell_loves` weighs
  favourites against `loves_told` per `ListeningService`, telling the difference either way, at most
  `LOVES_TOLD_AT_ONCE` per call. A love refused with any 4xx but 401, 403, 408, 429 is noted told so
  it cannot block the rest; any other failure returns at once, the love asked again. A favourite is
  a state: every one held when a token is first given is told. `Scrobbler::token_held` asks
  `validate-token` via `Client::json_as` (the one GET carrying an `Authorization`), answering
  `TokenHeld::By` the user, or `Unknown` (also for 400, 401, 403).
- **The binary's half is a thread following the file.** `submitting.rs` starts `resonate-submit` for
  the window and every playing command: after `FIRST_AFTER`, then every `SUBMITTED_EVERY`, it asks
  `Library::submit_listens`, doubling the wait per failure up to `WAITED_AT_MOST`; loves have their
  own `Pace`. Each service the settings hold an account for is a `Telling` of its own, so a refused
  Last.fm session holds back nothing told to ListenBrainz. Every `PLAYING_LOOKED_AT_EVERY` it tells
  a new row via `Library::billed_as` as playing now: once per row, not while paused, never for a
  file the catalog names nothing for; `lapses` tells again after a resume, a repeat or a seek back.
  The accounts follow the file as `Followed` does (`config::submitting_in` reads `online`,
  `listenbrainz-token` and the three `lastfm-` keys into `Accounts`); a client is built once per
  account. A 401 or 403 to listens or loves (`TOKEN_REFUSED`) holds that token back until the file
  names another.

## Last.fm

- **What was heard is scrobbled under a session the listener signed in for, signed by an
  application the listener registered.** `LastFm` is the second `Scrobbler`
  (`ListeningService::LastFm`; the catalog's `submissions` and `loves_told` rows are per service).
  Every call is a form POST to `Host::LastFm`: `api_sig` = MD5 of every field but `format`, sorted
  by name, written name then value, then the application's secret (`Fields::signature`),
  `format=json` after. `track.scrobble` one request per batch of at most `SCROBBLED_AT_ONCE` (50), the size `LastFm` answers `Scrobbler::at_once`, so the library marks the mark after each request, each listen numbered
  (`artist[i]`, `track[i]`, `timestamp[i]` when it began, …); `track.updateNowPlaying` for what
  plays; `user.getInfo` answers who holds the session. An error in the answer is read by its code
  (`refused`): invalid session, key or authentication = `Refused` 401 (the submitter holds that
  session back), rate limit 429, service down 503, the rest unreadable. **A favourite is loved there
  by its names**: `Library::tell_loves` hands each `Scrobbler::love` a `Loved`: the recording and the
  `LovedNames` (title, artist) of the catalog track carrying it (`THE_NAMES_OF_A_RECORDING`).
  Last.fm signs `track.love`/`track.unlove` with `artist` and `track`; a recording no track names any
  more is told nothing and marked told. ListenBrainz reads the recording alone.
- **A session is asked once, by name and password, and the password is not kept.**
  `resonate_online::signed_in` asks `auth.getMobileSession`; `resonate lastfm --user` reads the
  password from standard input (echo off on a terminal, `input::a_line_unechoed`) and stores only
  the session key (`lastfm-session`); `--forget` clears it. The window asks the same call through
  `Scrobblers::signed_in_to_lastfm` (a `LastfmSignIn` in, a `LastfmSession` out), which the binary's
  `ByToken` answers with `lastfm_signed_in`. These types, `lastfm::Application`,
  `lastfm::Session` and the window's `settings::Online` print no secret in `Debug`
  (`<withheld>` for every token, password, secret and the contact:
  `an_application_and_a_session_print_neither_secret_nor_session_key`,
  `the_online_settings_print_no_secret_and_no_contact`).

## Lyrics: LRCLIB and Better Lyrics, scored per song

- **Each song is asked of both services and the best answer kept** (`lyrics::told`). LRCLIB is
  asked first (`Service::ASKED_IN_TURN`), Better Lyrics after unless the first already scores
  `Score::BEST`. `Score` orders what a set gives, read off the set itself: `Detail` (unsynced,
  lines, words), then two singers over one, then sheet-given ends; a tie keeps the earlier service
  (`two_singers_word_by_word_outscore_one_and_words_outscore_lines`). A service that fails is
  logged and the next asked; the failure is returned only where nobody answered.
- **`OnlineLyrics` reads what the catalog kept before asking, and keeps what it is told.** It refuses a
  `Wanted` with no title or artist. `Library::kept_lyrics` is read first, keyed by the `Wanted`'s
  location and `Wanted::span`: a kept row not `KeptLyrics::is_due` answers without a request (its
  words, or nothing for a kept miss); a due one is asked again, the higher `Score` of kept and
  answer going to the pane (`richer`), a failed request falling back to the kept. No library: keeps
  nothing.
- **Window and lookup ask one way**: `lyrics::told` is the whole question, mapped by `OnlineLyrics`
  into a lyric error and by `Reference::lyrics` into the library's. **The kept document names its
  service** (`lyrics::source_of`, `resonate_online::lyric_source_of`): one starting with `<` is
  Better Lyrics' TTML, anything else LRCLIB's.
- **`/get` is exact, `/search` weighed.** 404 on `/get` falls through to `/search`; `pick` takes
  answers that `names` the track and `lasts_about` as long within `LENGTH_MAY_DIFFER_BY`, best by
  `Worded` (synced, plain, wordless, instrumental), then nearest length. An `instrumental` or
  wordless answer is `None`, kept as a miss. `LyricText` = `syncedLyrics` else `plainLyrics` (else
  the lyricsfile's lines), with the answer's `lyricsfile` beside it **only where that document says
  more than its lines** (one written from an LRC times no word and is the LRC again at four times
  the size).
- **A Lyricsfile is read by `resonate-lyrics`** (`lyrics.md`); LRCLIB's answer goes through the same
  `read_lyricsfile`, a kept document is read back through `read_document`, one no longer reading
  falls back to the kept LRC text.
- **Better Lyrics is asked `/getLyrics` by song, artist, album and whole seconds**
  (`betterlyrics::told`); it answers Apple's TTML, word-timed with singers more often than LRCLIB.
  Keyless, it answers only what it has cached: a `401` (uncached, a key wanted) reads as nothing
  known, like a 404, and is kept as a miss. No key is sent or offered. Paced at
  `BETTER_LYRICS_INTERVAL` (4 s; the service allows 15 a window per address, then `429` with
  `Retry-After`). An answer is passed over where its `score` (present on a fresh match only) is
  under `MATCHED_AT_LEAST`, or its `<body dur>` is another length than the track's
  (`LENGTH_MAY_DIFFER_BY`, the reader's). Kept as `LyricText`: the lines as LRC (`lrc_of`, a second
  singer's lines under `[v2: …]`) for search and fallback, and the TTML beside them where it says
  more than its lines.

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
  `shazam.com`, else `Heard::link` is `None`. The endpoint is undocumented, likeliest to change;
  `shazam_answers_a_clip_it_does_not_know_with_nothing_rather_than_a_refusal` is the live test that
  notices.
- **AudD gets the clip itself, only with a token the listener typed**: mono mix as 16-bit WAVE in
  `multipart/form-data`, refusing a clip past `LARGEST_CLIP`. `status` other than `success` =
  `Refused` with the service's error code; `null` result = nothing heard. `song_link` is kept only
  over https on `lis.tn`.
- **A file nothing else can name is named by ear, where the listener said so.** `ByEar` is a
  `Fingerprints` registered after `AcoustId` wherever `online` is on, answering only while the shared
  `identify-by-sound` switch is on (`Fingerprints::answers`, weighed by
  `Fingerprinters::has_a_source` and `recognise`: a switch off costs no study). It signs a
  twelve-second `resonate_analysis::excerpt` with `Shazam::signed`, then turns what Shazam names into
  recordings: its ISRC via `recordings_of_isrc`, takes within its own `LENGTHS_AGREE_WITHIN` of the
  file's length scored whole, else a `find_recording` phrase search. The pass weighs them as any
  recognition (`Certainty::Nearly`: a name the file never gave is filled, none it gave touched).

## Fixtures and the live test

**Mapping is proved over captured answers; services are reached only on request.**
`tests/fixtures/` holds a captured document per shape, pulled into each module's tests by
`include_str!`, so document-to-model runs on every `cargo test` with no network. Captured whole, not
cut down; the AudD ones were written from the service's documentation. `tests/live.rs` is gated on
`RESONATE_ONLINE_TESTS` and shares one client so pacing holds across its tests.

## Mobile composition

Android and iOS compose the same `Online`, LRCLIB and Better Lyrics, AutoEq, ListenBrainz, Last.fm, Shazam, AudD and
network-provider implementations through `resonate-mobile`, over its document catalog. It shares one paced client
for metadata services and follows that client's online gate. It never invents local paths for granted
document URIs or imports the PipeWire capture implementation. Android orchestration, account storage and
cache rules are in `android.md`.

## The binary's half

**`crates/resonate/src/online.rs` is the only file naming `resonate_online`'s types, and has a twin
for the build without it.** Under the feature: `reference` answers an `Online` only where
`Config::online_enabled`; `corrections` = `Corrected::uncorrected()` + an `AutoEq` under the same
condition; `lyricists` = `Lyricists::local_choosing_by_the_locale` + an `OnlineLyrics` (the last two get
the library where there is one); `fingerprinters` and `recognisers` register the printers and
recognisers above. `reference_asked_for` and `corrections_asked_for` = the same two or
`Error::OnlineOff` (the second asks `Corrected::has_a_source`). **An AutoEq answer is kept only once
it reads**: an index naming no device, or a profile `read_profile` makes no correction of (a captive
portal's page), is `Unreadable`, never cached for the index's 30 days or a device's 90. Without the
feature the twins answer `None`, `Error::NoReference`, `Corrected::uncorrected()` and the stubs, so
`main.rs` reads the same names either way. All is built over the one `Arc<Client>` in
`online::CLIENT` (made on first use over `INTRODUCTION`; `online::YIELDING` holds its yielding
twin): one agent and its connections per process.
