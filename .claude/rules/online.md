---
paths:
  - "crates/resonate-online/**/*.rs"
  - "crates/resonate/src/online.rs"
---

# The online crate

`resonate-online` is the one `Reference` the library's enrich pass can be handed, the one
`LyricProvider` that reaches a network and the one `Corrections` the equaliser's AutoEq search is
handed, and only the binary depends on it, behind the `online`
feature. It reaches `resonate-library` for the vocabulary it fills and the `LookupOp` it names,
`resonate-lyrics` for the provider seam, `resonate-eq` for the correction seam and its catalogue,
`resonate-codec` for `CoverArt` and `ImageFormat::sniff`,
and `resonate-core` for `SourceId`; nothing else in the workspace reaches it, so
`--exclude resonate-ui --no-default-features` builds with no HTTP client in the tree and
`cargo tree -p resonate-library` stays free of `ureq` and `serde`.

## The client

- **One `Client` serves ten hosts, and it says what this build is and nothing else.** `Host` is
  `MusicBrainz`, `CoverArtArchive`, `Commons`, `Wikidata`, `Lrclib`, `AutoEq`, `AcoustId`,
  `Shazam`, `Audd` and `AppleArtwork`, each with its base URL in `Host::base`. `AcoustId` is paced at `ACOUSTID_INTERVAL`, 334 ms, the
  three requests a second that service asks for, and is asked with the `acoustid-key` the listener
  registered and nothing else; `analysis.md` has how its answer is read. The last of them is `raw.githubusercontent.com/jaakkopasanen/AutoEq/master`, a
  file server rather than an API, which is why `eq.md` says the search that reads it lives in
  `resonate-eq`.
  `Identity::user_agent` is what every request carries: `resonate/<version>` from
  `CARGO_PKG_VERSION`, and ` ( <contact> )` after it only where `Identity::contact` holds text that
  is not blank. `Identity::of_this_build` carries no contact; the binary's `online::identity` fills
  it from `Config::contact`, which is the `contact` key of `config.toml` and nothing else, so a
  bare install identifies itself by name and version alone. `Online`, `Lrclib` and `AutoEq` each
  take an `Arc<Client>` — `Online::with_client`, `Lrclib::new` and `AutoEq::new` — so the three can
  share one, and `Online::new` is the convenience that builds its own.
- **A host is paced by reserving a slot, not by sleeping after a call.** `Client::pace` holds a
  `BTreeMap<Host, Instant>` behind a `parking_lot::Mutex`: the next slot is the later of now and
  the last slot plus the host's interval, written back under the lock and slept towards outside
  it, so two threads asking the same host queue behind each other rather than both measuring from
  one last call. `MUSICBRAINZ_INTERVAL` is one second, which is what that service asks of a client,
  `SHAZAM_INTERVAL` three seconds, because the service publishes no limit and a listener names a
  song at most every few seconds, `AUDD_INTERVAL` one second, and `OTHERS_INTERVAL` is 250 ms for
  the rest. The clock is a `Clock` trait — `WallClock`
  in a run, `Faked` under test through `Client::on_clock` — so the pacing is asserted on a fake
  clock's record of what it was asked to sleep rather than by timing a test.
- **A 503 or a 429 is asked three more times with the wait doubling, and `Retry-After` is read in
  seconds and capped.** `Client::fetch` retries `SERVICE_UNAVAILABLE` — MusicBrainz's answer to a
  client going too fast — and `TOO_MANY_REQUESTS`, HTTP's own word for it, up to `BUSY_RETRIES`,
  three, each after `cooling_off`: the header's integer seconds where the service names one, and otherwise a
  default that starts at `RETRY_AFTER_BY_DEFAULT` of 2 s and doubles per retry — 2 s, 4 s, 8 s —
  either capped at `RETRY_AFTER_AT_MOST` of 10 s, because a pass over a library must not park
  for the minutes a service may name. The fourth answer is returned whatever it says, so a
  service busy for a quarter of a minute costs one album its turn rather than the pass. It is
  proved against a scripted loopback `TcpListener` — `serving` answers each connection with the
  next status in its script and counts the connections — on the same fake clock the pacing is
  proved on, so what is asserted is how many times the socket was reached and what each sleep
  was, in `a_busy_service_is_asked_three_more_times_with_the_wait_doubling_between`,
  `a_fourth_refusal_is_returned_as_it_is` and
  `a_wait_the_service_names_is_taken_over_the_doubling_default`. `CONNECT_WITHIN` is 10 s and
  `ANSWER_WITHIN` is 30 s as the agent's `timeout_global`, so one unanswered request costs a pass
  half a minute rather than a run.
- **A status is read, never raised, and a body is read under a cap.** The agent is built with
  `http_status_as_error(false)`, so `Client::bytes` sees every status: a 404 is `Ok(None)`, which
  is what a release the service does not hold and a track LRCLIB has no words for both answer
  with, and any other failure is `Error::Refused { status }`. The body is read through
  `Body::as_reader` under a `take` one byte past the cap — `LARGEST_DOCUMENT` of 4 MiB for a JSON answer,
  `LARGEST_PICTURE` of 8 MiB for a cover or a portrait, `LARGEST_INDEX` of 2 MiB for AutoEq's
  `INDEX.md` and `resonate_eq::LARGEST_PROFILE` for one measurement — and a body past it is
  `Error::TooLarge` rather than a `Vec` the size a server chose. The cap is the caller's argument
  rather than the host's, because one host serves two shapes of answer and the index is the only
  megabyte-scale text this build reads. The cap is weighed against what the reader answers
  *after* the gzip decoder rather than against the socket, because ureq's own `limit` wraps the
  socket beneath the decoder and a body a thousand to one would have grown the `Vec` past it;
  `a_cap_bounds_the_body_as_it_is_decoded_rather_than_as_it_arrived` is the claim.
- **Nothing is asked over plain HTTP.** The agent is built `https_only`, so a redirect to a
  plain address is refused as `Unreachable` rather than followed, and `coverart::encrypted`
  upgrades the `http://` URLs the archive's index names — the same paths answer over HTTPS —
  because a cover lands in the catalog, the vault and, through `resonate tag --apply`, the files
  themselves, and every request names a release the listener holds. The tests' loopback servers
  are the one exception, which is what `Carried::Plain` is for, and it is compiled only for them. `Client::json` parses those bytes
  with `serde_json::from_slice` and answers `Error::Unreadable` where they are not the document
  expected, with the reason as a debug record rather than in the error.
- **A `ureq::Error` is classified once, into four shapes, and each caller reads them in its own
  vocabulary.** `Error::from_ureq` is the whole of it: `StatusCode` is `Refused`, `Io` is
  `Unreachable` carrying that error, a timeout, a host not found and every connect, proxy and TLS
  failure are `Unreachable` carrying an `io::Error` synthesised from the kind that names it,
  `BodyExceedsLimit` is `TooLarge`, and anything else is `Unreadable`. `From<Error> for
  resonate_library::Error` drops the host and folds `TooLarge` into `Unreadable`, so the library
  sees `Unreachable { op, source }`, `Refused { op, status }` and `Unreadable { op }` and can end a
  pass on the first and carry on past the other two; `Error::into_lyric_error` maps the same four
  onto `resonate_lyrics::Error::Unreachable { provider, cause }` and `Unreadable { provider, op }`.
  `size_of::<Error>()` is held under 128 bytes by the same guard test every crate carries.
- **A query is escaped once, in `query.rs`.** `escape_query` percent-escapes every byte outside
  the unreserved set, `lucene_quoted` wraps a term in double quotes and backslashes the quotes and
  backslashes inside it, and `Params` joins `name=value` pairs with `?` and `&` and leaves an absent
  value out, so a title carrying an ampersand or a quotation mark reaches a service as the words it
  is rather than as syntax.

## MusicBrainz

- **A release is asked for whole, with everything the enrich pass will store, in one request.**
  `musicbrainz::release` asks `/release/<mbid>` with `RELEASE_INCLUDES` — recordings,
  artist-credits, media, release-groups, isrcs, labels, url-rels and recording-level-rels — and
  `ReleaseDoc::into_release` maps it: media sorted by position and tracks within each by position,
  the first label-info's label and catalogue number, `has_front_cover` off the
  `cover-art-archive.front` flag, the release group's id and primary type, and links out of the
  url-rels through `Link::new`. A track's credit is its own where the document gives it one and
  the recording's otherwise, and it is kept as `ReleaseTrack::artist` only where the credited
  string differs from the release's `credited_as`, so a plain album's rows carry no artist and a
  split single's do. A track's length is its own or the recording's, its ISRC the recording's
  first, and its `number` the position spelled out where the document names none. An `id` that is
  not an mbid is `Error::Unreadable`, because a document that cannot name itself cannot be stored.
- **A search answers candidates, and the strict rule that takes one lives in the library.**
  `find_release` under `Wording::Phrase` writes a Lucene query naming only what the
  `ReleaseAsked` carries — `release:"…"`, then `credited_to`, which is `AND arid:<mbid>` where
  the owner's id is known and `AND artist:"…"` where only a name is, then `AND barcode:"…"` and
  `AND catno:"…"` where each is given, and no track count and no date, because the library
  weighs both itself and a query that names them hides the pressing one track wider — capped at
  `RELEASES_FOUND_AT_MOST` of ten, and `ReleaseFoundDoc::into_match` maps each hit to a
  `ReleaseMatch` with its score, title, the whole credit, its `release-group` id, track count and
  date, so the library can weigh any one credited artist and can reach the group of a hit whose
  count is wrong without a second search. Under `Wording::Words` the same endpoint is asked
  through `searched_in_words`: `query=<title> <artist>&dismax=true`, the title and the artist's
  name run together with no field at all, which is MusicBrainz's own loose search. `find_artist`
  asks `artist:"…"` for `ArtistMatch`es, capped at `FOUND_AT_MOST` of five. Neither weighs a
  hit: `enrich.rs` does, so what counts as a match is one rule in one crate rather than a rule
  per service. `a_release_query_names_only_what_was_asked`,
  `a_release_query_asks_by_the_artist_id_over_the_name_where_it_has_one` and
  `a_release_search_is_a_lucene_phrase_or_the_words_under_dismax` pin the three shapes byte for
  byte.
- **A recording is asked for three ways and every one of them maps through the same document.**
  `musicbrainz::recording` asks `/recording/<mbid>` with `RECORDING_INCLUDES` — artist-credits,
  releases, isrcs and media; `recordings_of_isrc` asks `/isrc/<code>` with the same includes and
  answers the whole `recordings` list, because one ISRC names every take released under it and
  telling them apart is the caller's; and `find_recording` writes `recording:"…"` with
  `credited_to` — `AND arid:<mbid>` or `AND artist:"…"` — `AND release:"…"` and
  `AND dur:[shortest TO longest]` where the `RecordingAsked` carries them, capped at
  `FOUND_AT_MOST` of five, which `a_recording_query_names_only_what_was_asked` pins. Under
  `Wording::Words` it takes the same `dismax` shape a release and a group search take, through
  `recording_search`: the title run together with the artist the ask names, or with the release
  where it names no artist, those being the two the library asks with and never both; the length
  and the fields are dropped with the phrase, a `dismax` query being words and nothing else.
  `a_recording_search_is_a_lucene_phrase_or_the_words_under_dismax` pins both shapes.
  **A fourth is the listener's own words.** `find_songs` is `Reference::find_songs`, what the
  window's search asks MusicBrainz for a song the catalog does not hold: the words run together
  under the same `dismax` shape, through `songs_search`, and capped at `SONGS_FOUND_AT_MOST` of
  twenty-five rather than five, because the library throws away every recording it already names
  and every second take of one title by one artist before it offers what is left.
  `a_song_is_searched_for_in_the_words_it_was_typed_in_under_dismax` pins it. Nothing asks by
  lyric text: LRCLIB's `q` searches the title, the artist and the album and nothing else, and no
  service that indexes the words themselves answers without a key.
  The duration window is
  `LENGTH_MAY_DIFFER_BY_MS`, ten seconds either way, and it is deliberately wider than the five
  seconds `enrich.rs` will accept: a query as narrow as the rule would drop the right take on the
  same second twice over, so the service is asked for the neighbourhood and the library decides
  inside it. `codes` drops anything the service calls an ISRC that `Isrc::new` will not hold.
- **A search answer is read for where each recording sits, because the index carries it.** A found
  recording maps its `releases` through the same `ReleaseOfRecordingDoc` a lookup does, so
  `RecordingMatch` reaches the library carrying its credit and its `RecordingRelease`s and
  `enrich.rs` can land one without asking `/recording` again. The search index spells a medium's
  track list `track` where a lookup spells it `tracks` — one `alias` on the field, not a second
  document — and gives no `position` inside it, so `placed` falls back to the medium's
  `track-offset` plus one, which names the same place. It carries `isrcs` too, spelled and shaped
  the way a lookup spells them, so `codes` reads them through the same filter and a search-
  identified track learns its code without a second request. A recording registered under none
  answers with none: four of the five takes in `recording_search.json` omit the key outright,
  which is what `a_search_answer_carries_the_codes_its_recording_is_registered_under` pins.
- **A recording is placed, and a release group is counted, and neither answer is the other's.**
  `/recording?inc=media` answers, for each release the recording sits on, only the medium the
  recording is *on* — one `media` entry holding the one track — so `placed` reads the disc and the
  position and nothing wider. That medium's own `track-count` is *not* read: disc 7 of a box set
  answers 6 rather than the box's hundred, so a count taken from there could never be weighed
  against an album's, and a field no caller may honestly read is one this tree does not carry.
  `/release-group?inc=media` is the other way round: every medium of every release comes back, so
  `counted` sums them into a `GroupRelease::track_count` that *is* the release's, which is exactly
  what `closest_release` needs. The captured `recording.json` is the proof of the placing: its
  box-set entry sits on disc 7 while the first of the twenty-five sits on disc 1.
- **A release group is asked for whole, and searched under `releasegroup:`.**
  `musicbrainz::release_group` asks `/release-group/<mbid>` with `RELEASE_GROUP_INCLUDES` —
  artist-credits, releases, media and url-rels — and maps the primary type, the first release
  date, the disambiguation, the credit, the links and every release under it as a
  `GroupRelease` with its title, its date, its country and its summed count.
  `find_release_group` under `Wording::Phrase` writes `releasegroup:"…"`, then `credited_to` and
  `AND firstreleasedate:YYYY*` where the `GroupAsked` gives them, and under `Wording::Words` the
  same `dismax` shape a release search takes, both capped at `FOUND_AT_MOST` of five, which
  `a_release_group_query_names_only_what_was_asked` and
  `a_release_group_search_is_a_lucene_phrase_or_the_words_under_dismax` pin. The field names are
  the whole of the care here: the release-group index spells them `releasegroup` and
  `firstreleasedate` where the release index spells them `release` and `date`, and a Lucene
  field a service does not know matches nothing rather than failing, so getting one wrong is a
  search that silently answers empty for ever.
- **An artist's release groups are browsed, not searched, and the browse is paged and capped.**
  `musicbrainz::release_groups_of` asks `/release-group?artist=<mbid>` — the browse endpoint,
  which answers everything the artist is credited on rather than the five best matches for a
  query — in pages of `BROWSE_PAGE`, 100, following `release-group-offset` and
  `release-group-count` until the count is reached or `GROUPS_AT_MOST`, 1 000, is, so a
  prolific artist costs ten requests and never more. `BrowsedGroupDoc::into_artist_release`
  maps each to an `ArtistRelease` with its title, primary type, secondary types and first
  release date, and a group naming no mbid is a debug record rather than a row; the library's
  `worth_keeping` is what reads the types, so the online crate keeps every group it is handed.
  `release_group_browse.json` is the first page of Pink Floyd's 651, and
  `a_release_group_browse_answers_one_page_of_an_artists_groups_with_their_types` reads all
  hundred of it.
- **An artist's profile, genres and links come out of one request too.** `musicbrainz::artist`
  asks `/artist/<mbid>` with `ARTIST_INCLUDES` — url-rels, tags and aliases — and maps the sort
  name, type, gender, country, area, begin area and life span, and its `genres` out of the tags
  with a positive count, sorted heaviest first and then by name, so a tag one editor added in
  jest does not outrank a hundred votes. `reference::portrait_urls` walks the links whose
  relation is `Relation::Image`, which is what the Commons fetch below is handed. The `aliases`
  the includes ask for are mapped by the function of that name, which keeps each spelling once by
  its `folded` form and drops any that folds to the name the artist is billed under, because an
  alias that *is* the name says nothing; `ArtistFoundDoc` maps them the same way, so a search hit
  carries them too and `enrich.rs`'s `names_it` has something to weigh below a real-name
  agreement.

## The Cover Art Archive

- **The archive is asked for its index, and the front picture is read at the size the window
  draws.** `coverart::cover` reads `/release/<mbid>` and, where that names no front, the
  `/release-group/<mbid>` index the release belongs to; `IndexDoc::front_url` takes the first
  image flagged `front` and prefers its `500` thumbnail, then `large`, then the full `image`, so
  a scan of a library fetches a screen's worth of pixels per album rather than the print master
  the archive also holds. `coverart::group_cover` is the half of that without a release to start
  from — the group index alone — because an album landed as its release group has no pressing to
  ask about, and both share `front_of` and `fetched` so there is one reading of an index and one
  fetch. The bytes are read under `LARGEST_PICTURE` and sniffed through
  `ImageFormat::sniff` in `coverart::picture`: bytes that are no picture are `Error::Unreadable`
  rather than a `CoverArt` the window would fail to decode later.

## Wikimedia Commons and Wikidata

- **A portrait is fetched from Commons alone, and the server does the scaling.**
  `commons::named` reads four URL shapes on `commons.wikimedia.org`, with or without `www.`, over
  `http` or `https`: a `File:` page, an `index.php?title=File:` one, a `Special:FilePath/` path
  that is *already* scaled — asked for again at the width this build wants rather than refused —
  and an `upload.wikimedia.org/wikipedia/commons/` original or thumbnail, whose last segment names
  the file. `commons::scaled` writes any of them as `Special:FilePath/<name>?width=600` —
  `PORTRAIT_WIDTH` — so the client never downloads an original that may run to tens of megabytes.
  Any other URL, a Wikipedia one included, answers `None` with a debug record, because an image
  relation can name anywhere and this build fetches only where the scaled path is known.
  **A portrait that comes back in a format `ImageFormat::sniff` cannot read is a miss**, where a
  cover's is `Error::Unreadable`: MediaWiki rasterises most of what `?width=` is asked of, but a
  file it answers in its own format is a legitimate answer about that file rather than a bad day,
  so `commons::drawable` passes it over with a debug record, the walk goes on to the next link, and
  nothing reaches the artist's refusal count.
- **Every `image` relation is tried, not the first.** `Reference::portrait` takes `&[Link]` rather
  than an `ArtistProfile` — the links are the whole of what it reads, which is what lets the
  library's retry hand it what the catalog holds without inventing a profile — and walks
  `portrait_urls` until one answers. An artist whose first image link is a shop's photograph used
  to resolve to nothing with a Commons one sitting behind it in the list.
- **Wikidata is followed for a picture where no image relation is held.** Most artists MusicBrainz
  knows carry a `wikidata` relation and no `image` one, and the entity behind it usually carries
  `P18`. `Host::Wikidata` is the sixth host, at the 250 ms `OTHERS_INTERVAL`, and `wikidata.rs`
  reads `Special:EntityData/<Q…>.json` for that one claim: the document is narrowed to `P18` by
  name, so serde parses nothing else in an entity however many properties it has, and the entities
  map is std's `HashMap` rather than an `AHashMap`, those keys coming off a network.
  `wikidata::entity` takes a `/wiki/Q…` or `/entity/Q…` URL and refuses anything that is not a
  `Q` followed by digits, so a `Property:` page names nothing. **The file name it answers is
  escaped on the way into a path**, through `query::escape_query`, and that is the difference
  between a portrait and nothing: a P18 value is a plain file name and about half of them carry
  spaces, which no server will read. `commons::file_path` does *not* escape, the name it cuts out
  of a URL already being escaped.
  Measured over a 435-track scan: 8 artists of 50 had a portrait before the three changes above
  and 25 after, with a second pass asking for nothing more.

## Deezer

- **A portrait nobody on Commons has taken is asked of Deezer, by the link MusicBrainz holds.**
  Composers and small acts rarely carry an `image` relation or a `P18`, but MusicBrainz links most
  of them to a Deezer artist page, and Deezer's public API answers that page's picture with no key.
  `Reference::portrait` walks the Commons links, then Wikidata, then `resonate_library::deezer_urls`
  — every link whose `Service` is Deezer — and `deezer::artist` reads the artist number out of a
  `deezer.com/[lang/]artist/<n>` URL and nothing else. `Host::Deezer` asks `/artist/<n>` and
  `picture_big`, 500 pixels square, is fetched from `Host::DeezerPictures` only where it is served
  from `*.dzcdn.net`; both are paced at `OTHERS_INTERVAL`. **Deezer's placeholder is no picture**:
  an artist with none answers a URL whose hash segment is `d41d8cd98f00b204e9800998ecf8427e`, the
  MD5 of nothing, and an unknown number answers a `DataException` document with no picture at all,
  so both are a miss and the walk goes on. A Deezer link counts towards `may_be_pictured`, so a
  catalog enriched before this is asked again by `look_again_for_portraits`. There is no search by
  name: a link MusicBrainz holds names the artist, and a name alone would be a guess.

## LRCLIB

- **`Lrclib` reads what the catalog kept before it asks, and keeps what it is told.** It is a
  `LyricProvider` named `lrclib`, and it refuses a `Wanted` naming no title or no artist, because
  the service has nothing else to search on. `Library::kept_lyrics` is read first, keyed by the
  `Wanted`'s location and `Wanted::span`: a kept text is read back through `read_lyrics` where it
  was synced and `Lyrics::plain` where it was not, without a request, and a kept miss — a row
  whose `text` is `NULL` — answers nothing while `still_fresh` under `ASK_AGAIN_AFTER` of seven
  days, so a track the service has no words for costs one request a week rather than one per play.
  A provider built with no library keeps nothing and asks every time.
- **`/get` is exact and `/search` is weighed.** `Lrclib::ask` asks `/get` with the track name,
  artist name and, where the `Wanted` has them, the album name and the duration in whole seconds;
  a 404 there falls through to `/search` on the title and artist, and `pick` takes the first
  answer that `names` the track — the title and the artist each folded to lowercase alphanumerics
  and compared for equality — and `lasts_about` as long, within `LENGTH_MAY_DIFFER_BY` of thirty
  seconds, the same tolerance `Sidecar` gives a `[length:]`. An answer is `Told::Synced` where it
  carries `syncedLyrics`, `Told::Plain` where only `plainLyrics`, and `Told::Nothing` where it is
  flagged `instrumental` or carries neither; the first two are kept with the text and the flag,
  and the third is kept as a miss, so an instrumental is remembered rather than searched for
  again. A failure on `/get` is a lyric error under `LyricOp::Fetch` and one on `/search` under
  `LyricOp::Search`, through `Error::into_lyric_error`.

## Recognising a clip

- **A clip is recognised by three services behind `resonate-listen`'s `Recogniser`, asked in
  order.** `online::recognisers` registers `Shazam` wherever `online` is on, `Audd` where the
  `audd-token` key holds one and `AcoustId` where `acoustid-key` does, and `Recognisers::recognise`
  takes the first that names the song; a silent clip is sent nowhere. A refusal is a `tracing`
  record and a name in `Recognition::refused`, not the end of the walk.
- **The client POSTs as well as GETs, through one exchange.** `Client::exchange` is the loop that
  paces, sends, and asks a 503 again, with a `Posted` body — its content type, whether it is
  `Encoded::Gzip` and the bytes — or none; `Client::posted` reads the answer under
  `LARGEST_DOCUMENT` into a document the way `json` does. A POST carries `Content-Language: en_US`
  beside the `Identity::user_agent` every request carries, and `Content-Encoding: gzip` where its
  body is packed, which only `Posted::packed_form` makes.
- **Shazam is asked with a signature, never with audio.** `Shazam::recognise` takes
  `resonate-analysis`'s `signature_of` over the clip's mono mix and POSTs it to
  `/discovery/v5/en/US/android/-/tag/<uuid>/<uuid>` with the query flags the endpoint expects: the
  two ids are fresh `uuid` v4s per request, the location is all zeros, the timezone is UTC and the
  user agent is this build's own, so nothing about the listener goes out but the peaks of twelve
  seconds of sound. An answer with an empty `matches` is nothing heard; a match is read for
  `track.title`, `subtitle` as the artist, the `SONG` section's *Album* and *Released*, the ISRC
  and the track's page, and its `coverarthq` is fetched from `AppleArtwork` — only where the URL is
  https and its host ends in `.mzstatic.com`, the one host the picture comes from — and handed on
  as a `Picture` sniffed by its first bytes. The endpoint is undocumented, so it is the service
  most likely to change under this build; `shazam_answers_a_clip_it_does_not_know_with_nothing_rather_than_a_refusal`
  is the live test that notices.
- **AudD is sent the clip itself, and only with a token the listener typed.** `Audd::recognise`
  writes the clip's mono mix as a 16-bit WAVE and POSTs it as `multipart/form-data` — the token,
  `return=apple_music,musicbrainz` and the file — under a boundary minted per request, refusing a
  clip past the service's 10 MB. An answer whose `status` is not `success` is `Refused` with the
  service's own error code; a `null` result is nothing heard. It is read for the title, artist,
  album, the year of `release_date`, the ISRC off Apple Music or MusicBrainz, the first MusicBrainz
  recording and the song link, and its Apple Music artwork is fetched at 600 pixels through the
  same `AppleArtwork` host.
- **AcoustID is asked about a clip with the print it would be asked about a file with.**
  `AcoustId` is a `Recogniser` as well as a `Fingerprints`: it prints the clip with `print_clip`,
  asks the same `/lookup` through `looked_up`, and names the clip by its best recording scored at
  `A_CLIP_HEARD_AT_LEAST`, 50. It is last because a snippet from the middle of a song rarely
  matches a print taken from its start.

## Fixtures and the live test

- **The mapping is proved over captured answers, and the services are reached only on request.**
  `tests/fixtures/` holds one captured document per shape — `release.json`,
  `release_linked.json`, `release_search.json`, `recording.json`, `isrc.json`,
  `recording_search.json`, `release_group.json`, `release_group_search.json`,
  `release_group_browse.json`, `artist.json`, `artist_search.json`, `coverart.json`,
  `wikidata.json`,
  `coverart_group.json`, `lrclib_get.json`, `lrclib_search.json`, `autoeq_index.md`,
  `autoeq_parametric.txt`, `shazam_match.json` and `shazam_nothing.json` — the last two captured
  from the live service with a signature this build took of a track in the library and of a run
  of synthetic notes — and `audd_recognised.json`, `audd_nothing.json` and `audd_refused.json`,
  written from the service's documentation because no token was to hand — pulled into each module's own tests with `include_str!`, so the walk
  from a document to a `Release`, a `Recording`, a `ReleaseGroup`, a page of `ArtistRelease`s,
  an `ArtistProfile`, a front URL, a `Told`, a `Catalogue` or a `Profile` runs on every
  `cargo test` with no network. They are captured whole rather than cut down, which is what
  lets a test assert over all twenty-five releases a recording names rather than over the first.
  `tests/live.rs` is the other
  half: gated on `RESONATE_ONLINE_TESTS` and printing a skip without it, it shares one client
  through a `OnceLock` so the pacing holds across its five tests, and asks the real services for
  the one release, one artist and one track the fixtures were captured from, for that artist's
  release groups — at least a hundred of them, *Meddle* among them — and for AutoEq's whole
  index and one measured correction.

## The binary's half

- **`crates/resonate/src/online.rs` is the only file that names `resonate_online`, and it has a
  twin for the build without it.** Under the feature, `reference` answers an `Online` over a fresh
  `Client` only where `Config::online_enabled`, `corrections` is `Corrected::uncorrected()` with an
  `AutoEq` appended under the same condition, and `lyricists` is `Lyricists::local()` with an
  `Lrclib` appended through
  `Lyricists::and`; the last two are handed the library where there is one, so the catalog keeps
  what each fetched. `reference_asked_for` and `corrections_asked_for` are the same two or
  `Error::OnlineOff`, the second asking `Corrected::has_a_source` rather than the config key again,
  because a build with the feature off has a `Corrected` that answers nothing. Without the feature
  the five answer `None`, `Error::NoReference`, `Corrected::uncorrected()`, `Error::NoReference`
  and `Lyricists::local()`, so `main.rs` reads the same five names
  either way — `lyricists` being the one of them compiled only into a `ui` build, in both halves
  alike, because nothing headless draws words. The feature is named in this file, on the two error
  variants and on
  `Config::online_enabled` and nowhere else. The
  reference, the correction source and the lyric provider are built over a client each rather than
  one between them, which costs nothing
  the pacing cares about, because the hosts each reaches do not overlap.
