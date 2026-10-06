---
paths:
  - "crates/resonate-online/**/*.rs"
  - "crates/resonate/src/online.rs"
---

# The online crate

`resonate-online` is the one `Reference` the enrich pass can be handed, the one `LyricProvider`
reaching a network, the one `Corrections` the AutoEq search is handed, and the printers,
recognisers and `Scrobbler` behind their seams. Only the binary depends on it, behind the `online`
feature, so `--exclude resonate-ui --no-default-features` builds with no HTTP client and
`cargo tree -p resonate-library` stays free of `ureq` and `serde`.

**Nothing about the listener leaves without their say.** `Config::online_enabled` (the `online`
key, default true) gates every request: `online::reference` answers `None` where it is off, and
`resonate enrich` then says `Error::OnlineOff` where a build without the feature says
`Error::NoReference`. `contact`, `acoustid-key`, `audd-token` and `listenbrainz-token` are empty by
default and set only by the listener: with no AcoustID key no fingerprint leaves the machine; AudD
is sent a clip only with a token; Shazam needs no key and is sent only a signature (the peaks of
what was heard), when a listener asks to listen or where `identify-by-sound` is on and a lookup has
nothing else to name a track by; ListenBrainz is the one sending something about the listener,
every counted play, and only under a token.

## The client

- **One `Client` serves every host and says what this build is and nothing else.**
  `Identity::user_agent_to` is `resonate/<version>`, with ` ( <contact> )` after it only where
  `Identity::contact` holds non-blank text and the host is one `Host::asks_who_is_asking`: the
  MetaBrainz hosts (MusicBrainz, Cover Art Archive, ListenBrainz) and Wikimedia's (Commons,
  Wikidata), whose policies ask for one. Every other host gets the bare name and version
  (`a_contact_is_told_to_the_hosts_that_ask_who_is_asking_and_no_other`). The binary's
  `online::identity` fills the contact from `Config::contact` alone.
- **What a client says, and whether it reaches the network, are read per request.** `Introduction`
  is the User-Agent behind a shared `RwLock`, written as the request's own header, so a contact
  typed into the settings is heard by every client sharing it from the next request
  (`online::INTRODUCTION`, `online::introduce`). `Introduction::following` carries a
  `Reintroduction` asked before each request; the binary's `Followed` reads the `contact` key alone
  (`config::contact_in`) only where the config file's modification time moved, keeping what was being
  said where the key will not read. `Client::reach` is a switch `exchange`
  reads before taking a turn: a client switched off answers `Error::Offline` having sent nothing,
  and `Setting::Online` stored in the window calls `online::reach`, so turning *Reach the network*
  off stops every service at once. Every seam's error reads `Offline` as `NetworkDown`.
- **A host is paced by reserving a slot, not by sleeping after a call, in one queue per process.**
  `Client::pace` reserves from a `Pacing` (a `BTreeMap<Host, Instant>` behind a `parking_lot::Mutex`):
  the next slot is the later of now and the last slot plus the host's interval, written back under
  the lock and slept towards outside it. Every client built by `Client::new` or `Client::introduced`
  shares `EVERY_CLIENT_IN_THE_PROCESS`, so the reference, `ByEar`, `AcoustId`, the recognisers and
  any other `Online` take turns at MusicBrainz in one queue; only a test's `Client::on_clock` gets
  its own. The `*_INTERVAL` constants in `client.rs` are the per-host decisions (MusicBrainz and
  AcoustID at the rates those services ask for). Pacing is asserted on a `Faked` `Clock`'s record of
  sleeps.
- **The listener goes first; a client made to yield waits for them.** A client is a `Listener` or,
  built by `Client::yielding`, its `Yielding` twin sharing the agent, the introduction, the queue and
  the `reach` switch, so *Reach the network* off stops both. A listener's `exchange` holds a
  `Listening` guard on the host from before its first slot through every busy retry, counted in
  `Turns::listening`; dropping it stamps `listener_answered` and wakes the `Paced::answered` condvar. A
  yielding caller waits on that condvar while any listener is counted, then asks only once the host
  is free — the later of the last slot plus the interval, any `cooling`, and the listener's answer plus
  the interval — and reserves nothing ahead, so a listener waits at most what is left of one
  interval and never queues behind the pass. The interval after the answer lets a listener's next
  ask (`find_albums` after `find_songs`, an artist page's next group) take the next slot. The cost is
  that a lookup pass stands still for as long as the listener keeps a host busy; that is meant
  (`a_listener_asks_back_to_back_while_a_yielding_client_waits_for_it`,
  `a_yielding_client_waits_a_whole_interval_after_the_listeners_answer`). The switch is re-read after
  the wait, so a client switched off while waiting sends nothing.
  The binary's `online::Asking` says which a caller is: `client_for(config, InTheBackground)` is
  the one yielding twin of the process client, and `reference`, `reference_asked_for` and
  `fingerprinters` take an `Asking`. A lookup pass is handed the yielding pair wherever it starts — the
  window's `Lookups::for_the_pass`, `resonate enrich`, the lookup after `resonate scan` and the MCP
  server — so its pictures and studies yield too; everything a listener waits on (a search, an
  artist's page, a want, a followed link, `share`, `analyse --recognise`) asks as the listener.
- **A busy answer is asked three more times with the wait doubling, and `Retry-After` is read in
  seconds and capped.** `busy` is 503 (MusicBrainz's answer to going too fast), 429, 502 and 504;
  `BUSY_RETRIES`, `RETRY_AFTER_BY_DEFAULT` doubling, `RETRY_AFTER_AT_MOST` since a pass must not
  park for the minutes a service may name. The fourth answer is returned whatever it says.
  **A host busy through every retry is then asked once, not four times, until it answers anything
  else**: `Pacing` notes it in `stayed_busy` and `retries_owed` answers none, so an album's ladder
  of rungs costs a request each against a service that is down, and the refusal reaches the library
  as `Refused` at once. **A `Retry-After` on the answer that is returned is still waited on**
  (`cooling`, taken by `reserve`). `CONNECT_WITHIN` and `ANSWER_WITHIN` bound a request so an unanswered
  one costs half a minute, not a run.
- **A status is read, never raised, and a body is read under a cap.** The agent is built
  `http_status_as_error(false)`: a 404 is `Ok(None)` (a release the service lacks, a track LRCLIB
  has no words for) and any other failure `Error::Refused { status }`. The body is read through
  `Body::as_reader` one byte past the caller's cap (`LARGEST_DOCUMENT`, `LARGEST_PICTURE`,
  `LARGEST_INDEX`, `resonate_eq::LARGEST_PROFILE`); past it is `Error::TooLarge`. The cap is the
  caller's argument, not the host's, and is weighed *after* the gzip decoder, since ureq's own
  `limit` wraps the socket beneath it and a thousand-to-one body would outgrow it
  (`a_cap_bounds_the_body_as_it_is_decoded_rather_than_as_it_arrived`). `Client::json` answers
  `Error::Unreadable` for a body that is not the expected document, the reason a debug record.
- **Nothing is asked over plain HTTP.** The agent is `https_only`, so a redirect to a plain address
  is `Unreachable`. Only the tests' loopback servers are plain (`Carried::Plain`,
  compiled for them alone).
- **A `ureq::Error` is classified once, into four shapes.** `Error::from_ureq`: a status is
  `Refused`; `Io`, a timeout, an unknown host and every connect, proxy and TLS failure are
  `Unreachable`; `BodyExceedsLimit` is `TooLarge`; anything else `Unreadable`.
  `From<Error> for resonate_library::Error` drops the host, so the library can end a pass on
  `Unreachable` and carry on past the others; `TooLarge` is warned of with its limit and stamped
  asked like a miss, counting toward no `REFUSALS_THAT_END_A_PASS`, since asking again would not
  shrink the document. `Error::into_lyric_error` maps the same four onto `resonate_lyrics::Error`.
- **A query is escaped once, in `query.rs`**: `escape_query`, `lucene_quoted` and `Params`, so a title
  with an ampersand or quotation mark reaches a service as words, not syntax.

## MusicBrainz

Searches answer candidates; the strict rule taking one lives in `enrich.rs`, so a match is one rule in
one crate, not one per service. The `Wording::Phrase` / `Wording::Words` pairs are Lucene phrases and
the loose `dismax` shape; the `a_*_query_names_only_what_was_asked` tests pin each byte for byte.

- **A search query names only what the ask carries.** A release query carries no track count or date,
  since the library weighs both and naming them hides the pressing one track wider; the artist is
  asked by `arid:` where its id is known. A release group is searched under `releasegroup:` and
  `firstreleasedate:`, where the release index spells `release` and `date`: a Lucene field a service
  does not know matches nothing rather than failing, a search silently empty for ever.
- **A release is asked for whole in one request** (`RELEASE_INCLUDES`), and mapped by
  `ReleaseDoc::into_release`. `stated` drops `[no label]` and `[none]`; a track's credit is kept as
  `ReleaseTrack::artist` only where it differs from the release's `credited_as`; an `id` that is not
  an mbid is `Error::Unreadable`.
- **A recording is asked three ways through one document**: by mbid, by ISRC (`/isrc/<code>`, whose
  `ISRC_INCLUDES` omit release-groups because that endpoint answers 400 with them, so a release read
  there carries its status alone; one ISRC names every take released under it and telling them apart
  is the caller's) and by search. The search's duration window is `LENGTH_MAY_DIFFER_BY_MS`, wider
  than the five seconds `enrich.rs` accepts, so the service is asked for the neighbourhood and the
  library decides inside it. `codes` drops anything `Isrc::new` will not hold.
- **The listener's own words** (`find_songs`, the window's search for a song the catalog lacks) are
  capped at `SONGS_FOUND_AT_MOST`, since the library throws away every recording it names and every
  second take before offering the rest. **Words read as a title by an artist are asked as that
  first**: `songs_by_search` requires the artist (each word of `SPELT_LOOSELY_FROM` letters or more
  spelt loosely) and lets the title only rank, falling back to the loose words on an empty answer.
  The title is not required because MusicBrainz tokenises it as entered, so a phrase misses *You F O*
  under `"you fo"`. **Words that read as no title by an artist are asked twice**: as the phrase an
  artist is credited under (`songs_credited_search`), whose answer leads, and as the loose dismax
  words. Dismax alone ranks a recording *titled* with the words (a cover, a mashup, *Twenty One
  Pilots* by someone else) above the band's own songs, so searching an artist's name found none
  of them. **`find_albums` asks the same two ways of `/release-group/`** (`albums_credited_search`,
  then `albums_search`, each `ALBUMS_FOUND_AT_MOST`), reading `primary-type`, `secondary-types` and
  `first-release-date` into an `AlbumMatch`; the library decides which are albums.
- **A search answer is read for where each recording sits.** The index spells a medium's track list
  `track` where a lookup spells `tracks` (one `alias`) and gives no `position`, so `placed` falls
  back to `track-offset` plus one; its `isrcs` are read through the same filter, so a
  search-identified track learns its code with no second request.
- **A recording is placed and a release group counted, and neither answer is the other's.**
  `/recording?inc=media` answers only the medium the recording is *on*, and that medium's
  `track-count` (disc 7 of a box set answers 6) is deliberately not read. `/release-group?inc=media`
  returns every medium of every release, so `counted` sums them into `GroupRelease::track_count`,
  which is the release's and what `closest_release` needs.
- **An artist's release groups are browsed, not searched, paged and capped.** `release_groups_of`
  uses the browse endpoint narrowed to `DISCOGRAPHY_KINDS`, in `BROWSE_PAGE` pages up to
  `GROUPS_AT_MOST`; what lies past is `Discography::unread` for the catalog to keep and the window to
  say, and a `from` reads the next stretch on request. Secondary types (compilation, live) stay the
  library's `worth_keeping` alone, so this crate keeps every group it is handed.
- **A group's songs are read off its official pressings, one request** (`releases_of_group`, at most
  `PRESSINGS_READ`; a pressing that will not map is left out). The browse caps a page at 500 tracks,
  so a box set answers fewer pressings, never none. Which pressing speaks for the group is the
  library's (`songs::pressing_of`).
- **An artist's pressings are browsed a page at a time, every group at once** (`releases_of_artist`:
  `/release?artist=…&status=official&type=album|ep|single` with the pressings' includes, 100 asked a
  page). The service cuts a page with recordings at about 500 tracks, so a page holds what fits —
  Daughter's 58 releases in one, twenty one pilots' 173 in four, The Beatles' 2 217 in 28 — and
  `ArtistPressings` carries `credited` (the `release-count`) and `read_to`, counted from the offset
  before any release that will not map is dropped, so the next page starts where this one did end.
  Whether one browse is worth more than a request a group is the library's (`learning.rs`).
- **A page is looked up for the artist MusicBrainz files it under** (`artist_at`:
  `/url?resource=<page>&inc=artist-rels`, the page escaped whole). A page MusicBrainz does not know is a
  404 and so `None`; one filed under two artists names neither (`LinkedUrlDoc::the_artist`). Which
  spelling of a page to ask is the library's (`ArtistLink::pages`).
- **An artist's profile, genres and links come from one request.** `genres` are tags with a positive
  count, heaviest first, so a single-editor tag does not outrank a hundred votes. `aliases` keep each
  spelling once by its `folded` form and drop any folding to the billed name. `portrait_urls` walks
  the `Relation::Image` links.

## Covers and portraits

**The archive is asked for the front at the size the window draws, in one request.**
`coverart::cover` asks `/release/<mbid>/front-500` and, where that answers nothing, the release group's
`/release-group/<mbid>/front-500`; the archive redirects to the image, so there is no index to read
first. A release with no 500 thumbnail yet is a miss until the next ask. Bytes are sniffed through
`ImageFormat::sniff`: no picture is `Error::Unreadable`, not a `CoverArt` the window would fail to
decode. The archive is paced at `COVER_ARCHIVE_INTERVAL` (100 ms), the window fetches six found-song
covers at once and the lookup reads pictures on four threads.

`Reference::portrait` takes `&[Link]`, so the library's retry can hand it what the catalog holds. It walks until one answers: every `image` relation (Commons), Wikidata,
Wikipedia, Apple Music, Spotify, Deezer, then SoundCloud. Every image relation is tried, since the
first may be a shop's photograph. A link to any source counts toward `may_be_pictured`, so a catalog
enriched before a source joined is asked again by `look_again_for_portraits`.

- **A refusal under 500 is a miss, not a refusal** (`passed_over_when_refused`): a page taken down
  or a CDN not serving a region is a fact about that link. 429 is not a miss, being a host asking
  for fewer requests, and leaves the artist asked again.
- **A source that fails does not end the walk.** `Walk::tried` keeps the first failure at or over
  500, unreadable or too large, and asks the next source; only where nothing answers is that failure
  what the library is told (`Walk::ended`). `Unreachable` alone ends the walk.
- **Commons alone, scaled by the server.** `commons::named` reads the `File:` page, `index.php?title=`,
  `Special:FilePath/` and `upload.wikimedia.org` shapes; `commons::scaled` writes
  `Special:FilePath/<name>?width=` at `PORTRAIT_WIDTH`, so no original is downloaded. Any other URL
  answers `None`. **A portrait in a format `ImageFormat::sniff` cannot read
  is a miss** where a cover's is `Error::Unreadable`: a file answered in its own format is a
  legitimate answer about that file, and nothing reaches the artist's refusal count
  (`commons::drawable`). `commons::file_path` does not escape, the name cut from a URL already being
  escaped.
- **Wikidata where no image relation is held.** Most artists carry a `wikidata` relation and an
  entity with `P18`. The document is narrowed to `P18` by name so serde parses nothing else, and the
  entities map is std's `HashMap` since its keys come off a network. `wikidata::entity` refuses
  anything not `Q` then digits. **A claim's rank is weighed**: `preferred`, then `normal`, never
  `deprecated`, which is how Wikidata keeps a picture it says is wrong. The answered file name is
  escaped through `query::escape_query` on the way into a path, about half carrying spaces.
- **A Wikipedia article leads to its entity** by `wbgetentities` on `Host::Wikidata` with the sitelink,
  `wikipedia::page` reading the language off the host (a mobile `m.` passed over, `zh-yue` written
  `zh_yue`) and the title off `/wiki/`.
- **Apple Music: the artist's own picture off the page MusicBrainz links**, its `og:image` read by
  `shared.rs`. `squared` asks mzstatic for a centred square crop. **A record sleeve is not a
  portrait**: where an artist has no picture Apple shows an album, so `squared` takes only a picture
  in an `AMCArtistImages…` or `Features…` bucket or named `pr_source`.
- **Spotify and SoundCloud: the page an artist keeps on a service**, again by `og:image`. The picture
  is taken only where it is an *artist* image (`i.scdn.co/image/ab676161…`, a sleeve being
  `ab67616d…`; an `avatars-…` file on `sndcdn.com`, the default avatar living elsewhere).
- **Deezer, by the link MusicBrainz holds**, its public API answering with no key; no search by name,
  a held link naming the artist and a name alone being a guess. **Deezer's placeholder is no
  picture**: a hash segment of `NOTHING_HASHED` (the MD5 of nothing) and the `DataException` document
  for an unknown number are misses. Deezer comes late because its picture CDN answers 403 to every
  request from some networks.
- Picture hosts are checked by `shared::on_host`: https, and the host is what stands before the first
  `/`, `?`, `#` or backslash, letters, digits, `-` and `.` alone.

## Links to a song, an album and a stream

- **`Reference::streamed_at` is `deezer::streamed`**, for a share the catalog holds no link for: by
  ISRC where there is one, otherwise (or on the unknown-artist `DataException`) a plain-words search,
  because Deezer's field syntax answers an empty list for every query now. A hit is taken only where
  title and artist fold to the asked ones through `folded_letters` and the length is within
  `LENGTHS_AGREE_WITHIN` where both are known, so an edit or live take is passed over; only a
  `https://www.deezer.com/` link is answered.
- **`Reference::song_linked` is `linked::named_at`**: what a pasted link names (ISRCs, length, title
  and artist as `LinkNames`), leaving the weighing to `Library::follow_link` (`library.md`). A Deezer
  track is asked of Deezer's API. **Every other service is read through song.link's page, not its
  API**, which answers every keyless request `401 PUBLIC_API_ACCESS_DEPRECATED`: `page_data` cuts
  the `__NEXT_DATA__` JSON out of `SongLink::page`, and `Song::read` takes an `entityData` only of
  `type` `song`. The page names an ISRC for Spotify, TIDAL and SoundCloud and none for Apple Music or
  YouTube, so the page's `deezer|song|<n>` twin is asked whenever there is one and its code added
  after the page's. A page naming no code still names a title and artist (what a YouTube upload is
  found by); only one naming neither a code nor both names is nothing.
- **`Reference::album_linked` is `linked::album_named_at`**, answering `AlbumNames` (the `Barcode`s the
  album is sold under) through album.link's page under the same `Host::SongLink` pace, and
  `releases_by_barcode` searches the release index's `barcode` under every spelling a leading zero
  makes of the code (`Barcode::spellings`), since MusicBrainz files a UPC-A as twelve digits or its
  thirteen-digit EAN. The Deezer twin's `upc` is added unless it is the same code a leading zero
  apart; both are offered, since Deezer may answer a reissue under another UPC. The library weighs
  the barcode rather than the score (`Library::follow_album_link`); a MusicBrainz link asks neither.
- **`Reference::artist_linked` is the name Deezer's `/artist/<n>` bills**, read off the same
  `ArtistDoc` a portrait is; a MusicBrainz artist link asks nothing, its id being in hand
  (`an_artist_page_names_the_artist_it_is_of_and_an_empty_answer_names_nobody`).

## ListenBrainz

- **What was heard is posted as JSON under the listener's token, and nothing else about them.**
  `ListenBrainz` is the `Scrobbler` the library's `submit_listens` is handed. `Posted::authorization`
  is the one header a body carries that others leave `None`; `listen_type` is `single` for one and
  `import` for several. A listen's moment is when it *began* (`listens.began`; the counted moment for
  a listen kept before the column), with `media_player`, `submission_client` and its version naming
  this build. A `status` other than `ok` is `Unreadable`; an HTTP status is `Refused` as for every
  host: the library tells a malformed batch (400) from a refused token (401) by it. **What is playing is told as it starts**:
  `Scrobbler::playing_now` posts a `playing_now` listen with no `listened_at`, which the service
  shows and never counts.
- **A favourite is a love, and a token is asked who holds it.** `Scrobbler::love` posts a `score` of
  1, or 0 for `Love::TakenBack` (no feedback, not a hate); a track with no recording MBID is never
  told. `Library::tell_loves` weighs favourites against `loves_told` per `ListeningService` and tells
  the difference either way, at most `LOVES_TOLD_AT_ONCE` a call so the submitter's look is not held
  a minute behind a first run. A love refused with any 4xx but 401, 403, 408 and 429 (the service
  saying this recording will never be taken) is noted as told so one cannot hold the rest back; any
  other failure returns at once and the same love is asked again. A favourite is a state, so every
  one held when a token is first given is told. `Scrobbler::token_held` asks
  `validate-token` through `Client::json_as`, the one GET carrying an `Authorization`, and answers
  `TokenHeld::By` the user or `Unknown`.
- **The binary's half is a thread following the file.** `submitting.rs` starts `resonate-submit` for
  the window and every playing command: after `FIRST_AFTER` and every `SUBMITTED_EVERY` it asks
  `Library::submit_listens`, doubling the wait after each failure up to `WAITED_AT_MOST`; loves have
  a `Pace` of their own, so a failing love backs off alone. Every `PLAYING_LOOKED_AT_EVERY` it looks
  at the player and tells a new row through `Library::billed_as` as playing now, once per row, not
  while paused, never for a file the catalog names nothing for; `lapses` tells it again after a
  resume, a repeat coming round or a seek back, so the service's *playing now* does not run out. Its token follows the file as `Followed` does
  (`config::submitting_in` reads `online` and `listenbrainz-token`), and the `ListenBrainz` client
  is built once per token so connections and pacing outlive the looks. A 401 or 403 to either
  listens or loves (`TOKEN_REFUSED`) holds that token back until the file names another. 

## LRCLIB

- **`Lrclib` reads what the catalog kept before it asks, and keeps what it is told.** It refuses a
  `Wanted` naming no title or artist. `Library::kept_lyrics` is read first, keyed by the `Wanted`'s
  location and `Wanted::span`: a kept row not `KeptLyrics::is_due` answers without a request (its
  words, or nothing for a kept miss); a due one is asked again, the richer of kept and answer going to
  the pane, a failed request falling back to what was kept. Built with no library it keeps nothing.
- **The window and the lookup ask one way**: `lrclib::told` is the whole question, mapped by `Lrclib`
  into a lyric error and by `Reference::lyrics` into the library's.
- **`/get` is exact, `/search` weighed.** A 404 on `/get` falls through to `/search`; `pick` takes
  answers that `names` the track (title and artist folded to lowercase alphanumerics) and
  `lasts_about` as long within `LENGTH_MAY_DIFFER_BY`, best by `Worded` (synced, plain, wordless,
  instrumental), then nearest in length. An `instrumental` answer or one with no words is `None`,
  kept as a miss. The `LyricText` is `syncedLyrics` else `plainLyrics`, with the answer's `lyricsfile`
  beside it **only where that document says more than its lines**: LRCLIB writes a Lyricsfile for
  every record, and one written from an LRC times no word and is the LRC again at four times the size.
- **A Lyricsfile is read by `resonate-lyrics`** (`lyrics.md` has the reader and the sidecar side);
  LRCLIB's answer goes through the same `read_lyricsfile`, and a kept document is read back through it,
  one no longer reading falling back to the kept LRC text.

## Recognising a clip

- **Two services behind `resonate-listen`'s `Recogniser`, asked in order.** `online::recognisers`
  registers `Shazam` wherever `online` is on and `Audd` where `audd-token` holds one, not AcoustID,
  which cannot match a clip (`analysis.md`). `Recognisers::recognise` takes the first that names the
  song; a silent clip is sent nowhere; a refusal is a `tracing` record and a name in
  `Recognition::refused`, not the end of the walk.
- **`Client::exchange` is the one loop for GET and POST**, with a `Posted` body or none. A POST
  carries `Content-Language: en_US`, and `Content-Encoding: gzip` only where `Posted::packed_form`
  made it.
- **Shazam is asked with a signature, never audio.** `Shazam::recognise` posts `signature_of` the
  clip's mono mix with fresh `uuid` v4 ids, a zeroed location, UTC and this build's agent. An empty
  `matches` is nothing heard. The cover (`coverarthq`) is fetched from `AppleArtwork` only over https on `mzstatic.com` or a
  subdomain (`shared::on_host`). The endpoint is undocumented and the likeliest to change;
  `shazam_answers_a_clip_it_does_not_know_with_nothing_rather_than_a_refusal` is the live test that
  notices.
- **AudD is sent the clip itself, only with a token the listener typed**: the mono mix as a 16-bit
  WAVE in `multipart/form-data`, refusing a clip past `LARGEST_CLIP`. A `status` other than `success`
  is `Refused` with the service's error code; a `null` result is nothing heard.
- **A file nothing else can name is named by ear, where the listener said so.** `ByEar` is a
  `Fingerprints` registered after `AcoustId` wherever `online` is on, answering only while the shared
  `identify-by-sound` switch is on (`Fingerprints::answers`, weighed by `Fingerprinters::has_a_source`
  and `recognise`, so a switch off costs no study). It signs a twelve-second `resonate_analysis::excerpt`
  with `Shazam::signed`, then turns what Shazam names into recordings: its ISRC through
  `recordings_of_isrc`, the takes within its own `LENGTHS_AGREE_WITHIN` of the file's length scored
  whole, otherwise a `find_recording` phrase search scored as MusicBrainz scored it. The pass weighs
  them as any recognition (`Certainty::Nearly`: a name the file never gave is filled and none it gave
  touched), which is what names a file with no title, no artist and a stem saying nothing.

## Fixtures and the live test

**The mapping is proved over captured answers; the services are reached only on request.**
`tests/fixtures/` holds a captured document per shape, pulled into each module's tests with
`include_str!`, so the walk from a document to its model runs on every `cargo test` with no network.
They are captured whole, not cut down; the AudD ones were written from the service's documentation.
`tests/live.rs` is gated on `RESONATE_ONLINE_TESTS` and shares one client so pacing holds across its
tests.

## The binary's half

**`crates/resonate/src/online.rs` is the only file naming `resonate_online`'s types, and it has a
twin for the build without it.** Under the feature, `reference` answers an `Online` only where
`Config::online_enabled`; `corrections` is `Corrected::uncorrected()` with an `AutoEq` appended under
the same condition; `lyricists` is `Lyricists::local()` with an `Lrclib` appended; the last two are
handed the library where there is one, so the catalog keeps what each fetched; `fingerprinters` and
`recognisers` register the printers and recognisers above. `reference_asked_for` and
`corrections_asked_for` are the same two or `Error::OnlineOff`, the second asking
`Corrected::has_a_source` rather than the key again. Without the feature the twins answer `None`,
`Error::NoReference`, `Corrected::uncorrected()` and the stubs, so `main.rs` reads the same names
either way. Everything is built over the one
`Arc<Client>` in `online::CLIENT`, made on first use over `INTRODUCTION`, so the process keeps one
agent and its connections.
