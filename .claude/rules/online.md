---
paths:
  - "crates/resonate-online/**/*.rs"
  - "crates/resonate/src/online.rs"
---

# The online crate

`resonate-online` is the one `Reference` the enrich pass can be handed, the one `LyricProvider`
reaching a network, the one `Corrections` the AutoEq search is handed, and the printers,
recognisers and `Scrobbler` behind their seams; only the binary depends on it, behind the `online`
feature. It reaches `resonate-library` for the vocabulary it fills and the `LookupOp` it names,
`resonate-lyrics` for the provider seam, `resonate-eq` for the correction seam and catalogue,
`resonate-codec` for `CoverArt` and `ImageFormat::sniff`, and `resonate-core` for `SourceId`;
nothing else reaches it, so `--exclude resonate-ui --no-default-features` builds with no HTTP
client and `cargo tree -p resonate-library` stays free of `ureq` and `serde`.

**Nothing about the listener leaves without their say.** `Config::online_enabled` (the `online`
key, default true) gates every request: `online::reference` answers `None` where it is off, and
`resonate enrich` then says `Error::OnlineOff` where a build without the feature says
`Error::NoReference`. `contact`, `acoustid-key`, `audd-token` and `listenbrainz-token` are empty by
default and set only by the listener: with no AcoustID key no fingerprint leaves the machine; AudD
is sent a clip only with a token; Shazam needs no key and is sent only a signature — the peaks of
what was heard — only when a listener asks to listen or where `identify-by-sound` is on and a
lookup has nothing else to name a track by; ListenBrainz is the one sending something about the
listener, every counted play, and only under a token.

## The client

- **One `Client` serves every host and says what this build is and nothing else.** `Host` is
  `MusicBrainz`, `CoverArtArchive`, `Commons`, `Wikidata`, `Lrclib`, `AutoEq`, `AcoustId`, `Shazam`,
  `Audd`, `AppleArtwork`, `AppleMusic`, `Spotify`, `SpotifyPictures`, `SoundCloud`,
  `SoundCloudPictures`, `Deezer`, `DeezerPictures` and `ListenBrainz`, each with its base URL in
  `Host::base`. `AcoustId` is paced at `ACOUSTID_INTERVAL` (334 ms, the three requests a second that
  service asks for) and asked with the listener's `acoustid-key` alone; `analysis.md` has how its
  answer is read. The `AutoEq` host is `raw.githubusercontent.com/jaakkopasanen/AutoEq/master`, a
  file server rather than an API, which is why the search reading it lives in `resonate-eq`
  (`eq.md`).
  `Identity::user_agent_to` is what every request carries: `resonate/<version>` from
  `CARGO_PKG_VERSION`, with ` ( <contact> )` after it only where `Identity::contact` holds non-blank
  text and the host is one `Host::asks_who_is_asking` — MusicBrainz, the Cover Art Archive and
  ListenBrainz, whose MetaBrainz policy asks for a contact, and Commons and Wikidata, whose
  Wikimedia one does. Shazam, AudD, AcoustID, LRCLIB, GitHub and the picture and artist services
  are told the build's name and version alone, a contact typed for MusicBrainz having once ridden on
  every request (`a_contact_is_told_to_the_hosts_that_ask_who_is_asking_and_no_other`). The agent's
  own configured User-Agent is the bare one too. `Identity::of_this_build` carries none; the binary's `online::identity` fills it from
  `Config::contact` — the `contact` key and nothing else, `None` for a blank value — so a bare
  install identifies itself by name and version alone. **What a client says is read per request.**
  `Introduction` is the User-Agent behind a shared `RwLock`; `Client::introduced` takes one and
  `Client::new` makes its own, and `exchange` writes it as the request's own `User-Agent` header
  (which ureq sends in place of the agent's configured one), so `Introduction::change_to` is heard by
  every client sharing it from their next request. The binary builds every client over one
  `online::INTRODUCTION` (made from the key on first use), and the settings file's `store` and
  `forget` of `Setting::Contact` call `online::introduce`, so a contact typed into the Online card
  needs no restart (`a_contact_given_after_the_client_was_built_is_what_the_next_request_says`). A
  contact written into the file by hand or by another process is heard likewise:
  `Introduction::following` carries a `Reintroduction` it asks before each request, and the
  binary's is `Followed`, which weighs the modification time of the file `Config::read_from` names
  against the last one seen and, only where it moved, reads the `contact` key alone through
  `config::contact_in` — so a stat is the whole cost where the file stood still, a key mistyped
  elsewhere costs the contact nothing, and a contact that will not read keeps what was being said
  rather than saying less. A run's first request reads it once, nothing having been seen.
  `an_introduction_that_follows_a_file_says_what_the_file_says_by_the_next_request` is the claim.
  `Online`, `Lrclib` and `AutoEq` each take an `Arc<Client>` — `Online::with_client`,
  `Lrclib::new`, `AutoEq::new` — so the three can share one; `Online::new` builds its own.
- **A host is paced by reserving a slot, not by sleeping after a call, in one queue per process.**
  `Client::pace` reserves from a `Pacing` — a `BTreeMap<Host, Instant>` behind a
  `parking_lot::Mutex` in an `Arc`: the next slot is the later of now and the last slot plus the
  host's interval, written back under the lock and slept towards outside it, so two threads asking
  one host queue behind each other rather than both measuring from one call. Every client
  `Client::new` or `Client::introduced` builds shares the one `EVERY_CLIENT_IN_THE_PROCESS`, so the
  reference, `ByEar`, `AcoustId`, the recognisers and any `Online` built beside them take turns at
  MusicBrainz in one queue rather than each in a slot of its own, together asking faster than the
  service allows (`every_client_in_the_process_takes_its_turn_in_one_queue_per_host`); only a
  test's `Client::on_clock` gets a queue of its own.
  `MUSICBRAINZ_INTERVAL` is 1 s (that service's ask), `SHAZAM_INTERVAL` 3 s (no published limit,
  and a listener names a song at most every few seconds), `AUDD_INTERVAL` 1 s, `OTHERS_INTERVAL`
  250 ms for the rest. The clock is a `Clock` trait — `WallClock` in a run, `Faked` under test
  through `Client::on_clock` — so pacing is asserted on the fake clock's record of what it was asked
  to sleep, not by timing a test. **The window's search waits its turn and says so.** A lookup's
  pass asks MusicBrainz one request at a time and reserves one slot at a time, so a song searched
  from the window takes the slot after the pass's — at most an interval and the request in flight,
  or a busy service's cooling-off — rather than a place behind a queue there is none of. The tracks
  pane's *Asking MusicBrainz…* heading reads `ASKING_BESIDE_A_LOOKUP` wherever the library is
  enriching, the one case the wait is longer than a request.
- **A 503, 429, 502 or 504 is asked three more times with the wait doubling, and `Retry-After` is
  read in seconds and capped.** `Client::exchange` retries what `busy` names —
  `SERVICE_UNAVAILABLE` (MusicBrainz's answer to a client going too fast), `TOO_MANY_REQUESTS`, and
  `BAD_GATEWAY` and `GATEWAY_TIMEOUT`, what a proxy in front of a service says for the same
  overload (`a_gateway_that_failed_or_timed_out_is_asked_again_like_a_busy_service`) — up to
  `BUSY_RETRIES` (3), each after `cooling_off`:
  the header's integer seconds where named, otherwise a default starting at
  `RETRY_AFTER_BY_DEFAULT` (2 s) and doubling per retry — 2, 4, 8 s — either capped at
  `RETRY_AFTER_AT_MOST` (10 s), since a pass must not park for the minutes a service may name. The
  fourth answer is returned whatever it says, so a service busy for a quarter-minute costs one album
  its turn, not the pass. **A host busy through every retry is then asked once, not four times,
  until it answers anything else**: the shared `Pacing` notes it in `stayed_busy`, and
  `retries_owed` answers none for it, so the rest of an album's ladder — release group, phrase,
  words — costs a request a rung against a service that is down rather than fourteen seconds a rung,
  and the refusal reaches the library as `Refused` at once
  (`a_host_busy_through_every_retry_is_asked_once_until_it_answers`). The first answer that is not
  busy clears the mark. Proved against a scripted loopback `TcpListener` — `serving` answers each
  connection with the next status in its script and counts connections — on the same fake clock,
  asserting how often the socket was reached and each sleep:
  `a_busy_service_is_asked_three_more_times_with_the_wait_doubling_between`,
  `a_fourth_refusal_is_returned_as_it_is` and
  `a_wait_the_service_names_is_taken_over_the_doubling_default`. `CONNECT_WITHIN` is 10 s and
  `ANSWER_WITHIN` 30 s as the agent's `timeout_global`, so an unanswered request costs half a minute,
  not a run.
- **A status is read, never raised, and a body is read under a cap.** The agent is built
  `http_status_as_error(false)`, so `Client::bytes` sees every status: a 404 is `Ok(None)` — what a
  release the service lacks and a track LRCLIB has no words for answer — and any other failure
  `Error::Refused { status }`. The body is read through `Body::as_reader` under a `take` one byte
  past the cap — `LARGEST_DOCUMENT` (4 MiB) for JSON, `LARGEST_PICTURE` (8 MiB) for a cover or
  portrait, `LARGEST_INDEX` (2 MiB) for AutoEq's `INDEX.md`, `resonate_eq::LARGEST_PROFILE` for a
  measurement — and a body past it is `Error::TooLarge`, not a `Vec` the size a server chose. The
  cap is the caller's argument, not the host's, since one host serves two shapes and the index is
  the only megabyte-scale text read. It is weighed against what the reader answers *after* the gzip
  decoder, not the socket, since ureq's own `limit` wraps the socket beneath the decoder and a
  thousand-to-one body would have grown the `Vec` past it
  (`a_cap_bounds_the_body_as_it_is_decoded_rather_than_as_it_arrived`).
- **Nothing is asked over plain HTTP.** The agent is `https_only`, so a redirect to a plain address
  is refused as `Unreachable`, and `coverart::encrypted` upgrades the `http://` URLs the archive's
  index names (the same paths answer over HTTPS), because a cover lands in the catalog, the vault
  and, through `resonate tag --apply`, the files, and every request names a release the listener
  holds. The tests' loopback servers are the one exception — `Carried::Plain`, compiled only for
  them. `Client::json` parses the bytes with `serde_json::from_slice` and answers `Error::Unreadable`
  where they are not the expected document, the reason a debug record rather than in the error.
- **A `ureq::Error` is classified once, into four shapes, each caller reading them in its own
  vocabulary.** `Error::from_ureq`: `StatusCode` is `Refused`; `Io` is `Unreachable` carrying that
  error; a timeout, a host not found and every connect, proxy and TLS failure are `Unreachable`
  carrying an `io::Error` synthesised from the kind naming it; `BodyExceedsLimit` is `TooLarge`;
  anything else `Unreadable`. `From<Error> for resonate_library::Error` drops the host and folds
  `TooLarge` into `Unreadable`, so the library sees `Unreachable { op, source }`,
  `Refused { op, status }` and `Unreadable { op }` and can end a pass on the first and carry on past
  the others; `Error::into_lyric_error` maps the same four onto
  `resonate_lyrics::Error::Unreachable { provider, cause }` and `Unreadable { provider, op }`.
  `size_of::<Error>()` is held under 128 bytes by the usual guard test.
- **A query is escaped once, in `query.rs`.** `escape_query` percent-escapes every byte outside the
  unreserved set, `lucene_quoted` wraps a term in double quotes and backslashes the quotes and
  backslashes inside, and `Params` joins `name=value` pairs with `?` and `&`, leaving an absent value
  out, so a title with an ampersand or quotation mark reaches a service as words, not syntax.

## MusicBrainz

- **A release is asked for whole, with everything the pass stores, in one request.**
  `musicbrainz::release` asks `/release/<mbid>` with `RELEASE_INCLUDES` — recordings,
  artist-credits, media, release-groups, isrcs, labels, url-rels and recording-level-rels — and
  `ReleaseDoc::into_release` maps it: media sorted by position and tracks within each by position;
  the first label-info's label and catalogue number, read through `stated`, since MusicBrainz
  writes `[no label]` and `[none]` for neither, which are no label and no number rather than a name
  to print or tag; `has_front_cover` off the `cover-art-archive.front` flag; the release group's id
  and primary type; and links out of the url-rels through `Link::new`. A track's credit is its own
  where the document gives one, else the recording's, and kept as `ReleaseTrack::artist` only where
  it differs from the release's `credited_as`, so a plain album's rows carry no artist and a split
  single's do. A track's length is its own or the recording's, its ISRC the recording's first, its
  `number` the position spelled out where the document names none. An `id` that is not an mbid is
  `Error::Unreadable` — a document that cannot name itself cannot be stored.
- **A search answers candidates, and the strict rule taking one lives in the library.**
  `find_release` under `Wording::Phrase` writes a Lucene query naming only what `ReleaseAsked`
  carries — `release:"…"`, then `credited_to` (`AND arid:<mbid>` where the owner's id is known,
  `AND artist:"…"` where only a name is), then `AND barcode:"…"` and `AND catno:"…"` where given —
  and no track count or date, since the library weighs both itself and a query naming them hides
  the pressing one track wider; capped at `RELEASES_FOUND_AT_MOST` (10), and
  `ReleaseFoundDoc::into_match` maps each hit to a `ReleaseMatch` with score, title, the whole
  credit, its `release-group` id, track count and date, so the library can weigh any one credited
  artist and reach the group of a hit whose count is wrong without a second search. Under
  `Wording::Words` the same endpoint is asked through `searched_in_words`:
  `query=<title> <artist>&dismax=true`, title and artist run together with no field — MusicBrainz's
  own loose search. `find_artist` asks `artist:"…"` for `ArtistMatch`es, capped at `FOUND_AT_MOST`
  (5). Neither weighs a hit: `enrich.rs` does, so a match is one rule in one crate, not one per
  service. `a_release_query_names_only_what_was_asked`,
  `a_release_query_asks_by_the_artist_id_over_the_name_where_it_has_one` and
  `a_release_search_is_a_lucene_phrase_or_the_words_under_dismax` pin the three shapes byte for byte.
- **A recording is asked for three ways, all mapping through one document.**
  `musicbrainz::recording` asks `/recording/<mbid>` with `RECORDING_INCLUDES` — artist-credits,
  releases, isrcs, media and release-groups, the last putting each release's group, with its primary
  and secondary types, beside the release's status, so `RecordingRelease::issued` says whether a
  release is an official album, a single, a compilation or a bootleg. `recordings_of_isrc` asks
  `/isrc/<code>` with `ISRC_INCLUDES` — the same less release-groups, which that endpoint answers
  with a 400, so a release read there carries its status alone — and answers the whole `recordings`
  list, one ISRC naming every take released under it and telling them apart being the caller's.
  `find_recording` writes `recording:"…"` with `credited_to`, `AND release:"…"` and
  `AND dur:[shortest TO longest]` where `RecordingAsked` carries them, capped at `FOUND_AT_MOST`
  (`a_recording_query_names_only_what_was_asked`); under `Wording::Words` it takes the `dismax` shape
  through `recording_search`: the title run together with the ask's artist, or with the release where
  it names none (the library never asks with both), length and fields dropped with the phrase
  (`a_recording_search_is_a_lucene_phrase_or_the_words_under_dismax`). **A fourth is the listener's
  own words**: `find_songs` is `Reference::find_songs`, what the window's search asks for a song the
  catalog lacks — the words under the same `dismax` shape through `songs_search`, capped at
  `SONGS_FOUND_AT_MOST` (25, not 5), since the library throws away every recording it names and
  every second take of one title by one artist before offering the rest
  (`a_song_is_searched_for_in_the_words_it_was_typed_in_under_dismax`). Nothing asks by lyric text:
  LRCLIB's `q` searches title, artist and album only, and no service indexing the words answers
  without a key. The duration window is `LENGTH_MAY_DIFFER_BY_MS`, ten seconds either way, wider
  than the five `enrich.rs` (`RECORDING_MAY_DIFFER_BY`) accepts: a query as narrow as the rule
  would drop the right take on the same second twice over, so the service is asked for the
  neighbourhood and the library decides inside it. `codes` drops anything the service calls an ISRC
  that `Isrc::new` will not hold.
- **A search answer is read for where each recording sits, the index carrying it.** A found
  recording maps its `releases` through the lookup's `ReleaseOfRecordingDoc`, so a `RecordingMatch`
  carries its credit and `RecordingRelease`s and `enrich.rs` can land one without asking
  `/recording` again. The index spells a medium's track list `track` where a lookup spells `tracks`
  — one `alias` on the field — and gives no `position` in it, so `placed` falls back to the medium's
  `track-offset` plus one. It carries `isrcs` too, shaped as a lookup's, read through the same
  filter, so a search-identified track learns its code with no second request; a recording
  registered under none answers none — four of the five takes in `recording_search.json` omit the
  key (`a_search_answer_carries_the_codes_its_recording_is_registered_under`).
- **A recording is placed and a release group counted, and neither answer is the other's.**
  `/recording?inc=media` answers, per release, only the medium the recording is *on* — one `media`
  entry holding the one track — so `placed` reads disc and position and nothing wider. That medium's
  own `track-count` is *not* read: disc 7 of a box set answers 6, not the box's hundred, so it could
  never be weighed against an album's, and a field no caller may honestly read is one this tree does
  not carry. `/release-group?inc=media` is the other way round: every medium of every release comes
  back, so `counted` sums them into a `GroupRelease::track_count` that *is* the release's — what
  `closest_release` needs. The captured `recording.json` proves the placing: its box-set entry sits
  on disc 7, the first of the twenty-five on disc 1.
- **A release group is asked for whole, and searched under `releasegroup:`.**
  `musicbrainz::release_group` asks `/release-group/<mbid>` with `RELEASE_GROUP_INCLUDES` —
  artist-credits, releases, media and url-rels — mapping the primary type, first release date,
  disambiguation, credit, links and every release under it as a `GroupRelease` with title, date,
  country and summed count. `find_release_group` under `Wording::Phrase` writes `releasegroup:"…"`,
  then `credited_to` and `AND firstreleasedate:YYYY*` where the `GroupAsked` gives them, and under
  `Wording::Words` the release search's `dismax` shape, both capped at `FOUND_AT_MOST`
  (`a_release_group_query_names_only_what_was_asked`,
  `a_release_group_search_is_a_lucene_phrase_or_the_words_under_dismax`). The field names are all
  the care: the release-group index spells them `releasegroup` and `firstreleasedate` where the
  release index spells `release` and `date`, and a Lucene field a service does not know matches
  nothing rather than failing — a search silently answering empty for ever.
- **An artist's release groups are browsed, not searched, paged and capped.**
  `musicbrainz::release_groups_of` asks `/release-group?artist=<mbid>&type=album|ep|single` — the
  browse endpoint, answering everything the artist is credited on rather than the five best matches,
  narrowed to `DISCOGRAPHY_KINDS`, the three primary types the library's `worth_keeping` keeps, so a
  broadcast costs no page and no cap — in pages of `BROWSE_PAGE` (100), following
  `release-group-offset` and `release-group-count` until the count or `GROUPS_AT_MOST` (1 000) is
  reached, so a prolific artist costs ten requests a pass at most; what lies past is answered as
  `Discography::unread` for the catalog to keep and the window to say, beside `Discography::read_to`,
  the offset reached. It takes a `from`, so the next thousand are read from there when the listener
  asks. `BrowsedGroupDoc::into_artist_release` maps each to an `ArtistRelease` with title, primary
  and secondary types and first release date; a group naming no mbid is a debug record. The library's
  `worth_keeping` still reads the types — the secondary ones (compilation, live) are its alone — so
  the online crate keeps every group it is handed. `release_group_browse.json` is the first page of
  Pink Floyd's 651, and
  `a_release_group_browse_answers_one_page_of_an_artists_groups_with_their_types` reads all hundred.
- **An artist's profile, genres and links come from one request.** `musicbrainz::artist` asks
  `/artist/<mbid>` with `ARTIST_INCLUDES` — url-rels, tags and aliases — mapping sort name, type,
  gender, country, area, begin area and life span, and `genres` from the tags with a positive count,
  heaviest first then by name, so a jesting single-editor tag does not outrank a hundred votes.
  `resonate_library::portrait_urls` walks the links whose relation is `Relation::Image`, what the
  Commons fetch is handed. `aliases` keeps each spelling once by its `folded` form and drops any
  folding to the billed name (an alias that *is* the name says nothing); `ArtistFoundDoc` maps them
  the same, so a search hit carries them and `enrich.rs`'s `names_it` has something to weigh below a
  real-name agreement.

## The Cover Art Archive

**The archive is asked for its index, and the front read at the size the window draws.**
`coverart::cover` reads `/release/<mbid>` and, where that names no front, the
`/release-group/<mbid>` index; `IndexDoc::front_url` takes the first image flagged `front` and
prefers its `500` thumbnail, then `large`, then the full `image`, so a scan fetches a screen's worth
of pixels per album, not the print master. `coverart::group_cover` is the half without a release —
the group index alone — for an album landed as its release group, with no pressing to ask about;
both share `front_of` and `fetched`, one reading of an index and one fetch. Bytes are read under
`LARGEST_PICTURE` and sniffed through `ImageFormat::sniff` in `coverart::picture`: bytes that are no
picture are `Error::Unreadable`, not a `CoverArt` the window would fail to decode.

## Portraits

`Reference::portrait` takes `&[Link]` rather than an `ArtistProfile` — the links are all it reads,
which lets the library's retry hand it what the catalog holds without inventing a profile — and
walks, until one answers: the `image` relations (Commons), Wikidata, Wikipedia, Apple Music,
Spotify, Deezer (`resonate_library::deezer_urls`), then SoundCloud. Every `image` relation is tried,
not only the first: an artist whose first image link was a shop's photograph resolved to nothing
with a Commons one behind it. A link to any of these counts towards `may_be_pictured`, so a catalog
enriched before a source joined is asked again by `look_again_for_portraits`. A picture refused with
a status under 500 is a miss, not a refusal (`passed_over_when_refused`): a page taken down or a CDN
not serving a region is a fact about that link, not a bad day to count against the artist.
**A source that fails does not end the walk.** Every step goes through `Walk::tried`: a refusal
under 500 is that miss; a refusal at or over it, an unreadable answer or one too large is kept as
the walk's first failure and the next source asked, so a Commons, Wikidata or Wikipedia having a
bad day does not hide the Apple, Spotify, Deezer or SoundCloud picture behind it; and only where
nothing answers is that first failure what the library is told (`Walk::ended`), so the refusal is
still counted against the artist. `Unreachable` alone ends the walk, since the library ends the pass
on it anyway (`a_source_that_fails_is_passed_over_and_the_next_is_asked`,
`a_walk_that_found_nothing_answers_its_first_refusal_and_a_miss_under_five_hundred_is_none`).

- **Commons alone, scaled by the server.** `commons::named` reads four URL shapes on
  `commons.wikimedia.org`, with or without `www.`, over `http` or `https`: a `File:` page, an
  `index.php?title=File:` one, a `Special:FilePath/` path *already* scaled (asked again at this
  build's width rather than refused), and an `upload.wikimedia.org/wikipedia/commons/` original or
  thumbnail, whose last segment names the file. `commons::scaled` writes any as
  `Special:FilePath/<name>?width=600` (`PORTRAIT_WIDTH`), so no original of tens of megabytes is
  downloaded. Any other URL, Wikipedia's included, answers `None` with a debug record, an image
  relation being able to name anywhere. **A portrait in a format `ImageFormat::sniff` cannot read is
  a miss**, where a cover's is `Error::Unreadable`: MediaWiki rasterises most of what `?width=` asks,
  and a file answered in its own format is a legitimate answer about that file, so
  `commons::drawable` passes it over, the walk goes on, and nothing reaches the artist's refusal
  count.
- **Wikidata where no image relation is held.** Most artists carry a `wikidata` relation and no
  `image` one, and the entity usually carries `P18`. `Host::Wikidata` is paced at `OTHERS_INTERVAL`,
  and `wikidata.rs` reads `Special:EntityData/<Q…>.json` for that one claim: the document is
  narrowed to `P18` by name, so serde parses nothing else however many properties an entity has, and
  the entities map is std's `HashMap`, those keys coming off a network. `wikidata::entity` takes a
  `/wiki/Q…` or `/entity/Q…` URL and refuses anything not `Q` then digits, so a `Property:` page
  names nothing. **A claim's rank is weighed**: `ClaimsDoc::best_picture` takes the first `P18`
  ranked `preferred`, then the first `normal` (an unknown rank read as normal), and never a
  `deprecated` one, which is how Wikidata keeps a picture it says is wrong
  (`a_preferred_picture_is_taken_over_an_earlier_one_and_a_deprecated_one_never`); the Wikipedia
  route reads its entity through the same. **The answered file name is escaped on the way into a path** through
  `query::escape_query` — the difference between a portrait and nothing, a P18 value being a plain
  file name and about half carrying spaces. `commons::file_path` does *not* escape, the name it cuts
  from a URL already being escaped. Measured over a 435-track scan: 8 artists of 50 had a portrait
  before every-link-tried, the drawable miss and Wikidata, 25 after, with a second pass asking
  nothing more.
- **A Wikipedia article leads to its entity.** An artist linked to an article and no entity is
  asked about through the article: `wikipedia::page` reads the language off the host (a mobile
  link's `m.` passed over, `zh-yue` written `zh_yue`) and the title off `/wiki/`, percent-decoded,
  and `wbgetentities` on `Host::Wikidata`, with `sites` the language's wiki and `titles` the article,
  answers the entity with that sitelink in the same narrowed `EntityDoc`, its `P18` going through
  `commons::scaled`. A Wikimedia host, not a new one. `wikipedia.json` is a captured answer for
  *Pink Floyd* on `enwiki`, narrowed to the claim read.
- **Apple Music: the artist's own picture off the page MusicBrainz links.** Most composers and small
  acts carry no Commons picture or `P18`, but nearly all are linked to an Apple Music artist page,
  which shares the chosen picture as its `og:image` from `mzstatic.com`. `apple::artist_page` reads
  a `music.apple.com` or `itunes.apple.com` link of any storefront — `artist/<id>`,
  `artist/<slug>/<id>` or `artist/id<id>` — into `https://music.apple.com/<store>/artist/<id>`,
  `Host::AppleMusic` fetches it, `shared_picture` cuts the `og:image` out, and `squared` asks
  mzstatic for `600x600cc.jpg` (cropped square at the centre) rather than the 1200×630 card. **A
  record sleeve is not a portrait**: where an artist has no picture Apple shows an album, filed under
  a `Music…` bucket and named by its barcode, so `squared` takes only a picture in an
  `AMCArtistImages…` or `Features…` bucket or named `pr_source` (an artist's own upload). Measured on
  this library it took portraits from 25 of 50 to 35, Adam Skorupa, Mikolai Stroinski and Piotr
  Musiał among them.
- **Spotify and SoundCloud: the page an artist keeps on a service**, each sharing the chosen picture
  as its `og:image`, read through `shared.rs` (the `og:image` cutter `apple.rs` once kept to itself)
  with the page under `LARGEST_DOCUMENT` and the picture under `LARGEST_PICTURE`, sniffed as every
  picture is. `spotify::artist` reads the twenty-two-letter id from an
  `open.spotify.com/[intl-xx/]artist/<id>` link and asks `Host::Spotify`; the picture is fetched from
  `Host::SpotifyPictures` only where it is an *artist* image — `i.scdn.co/image/ab676161…`, a
  sleeve being `ab67616d…` — the rule `squared` keeps for Apple. `soundcloud::user` takes a
  `soundcloud.com/<account>` link with nothing under it, and the picture only where it is an
  `avatars-…` file on `sndcdn.com` (an account's own upload; the default avatar lives elsewhere and is
  a miss). Both paced at `OTHERS_INTERVAL`. Measured on this library they took the identified artists
  no other source pictured from seven to three.
  `an_artist_linked_only_to_spotify_or_soundcloud_is_pictured_by_the_page_it_is_linked_to` is the
  live test.
- **Deezer, by the link MusicBrainz holds**, its public API answering an artist page's picture with
  no key. `deezer::artist` reads the artist number out of a `deezer.com/[lang/]artist/<n>` URL and
  nothing else; `Host::Deezer` asks `/artist/<n>`, and `picture_big` (500 px square) is fetched from
  `Host::DeezerPictures` only where served from `*.dzcdn.net`; both at `OTHERS_INTERVAL`. **Deezer's
  placeholder is no picture**: an artist with none answers a URL whose hash segment is
  `d41d8cd98f00b204e9800998ecf8427e`, the MD5 of nothing, and an unknown number a `DataException`
  document with no picture; both are misses. No search by name: a held link names the artist, a name
  alone would be a guess. Deezer comes late because its picture CDN answers 403 to every request from
  some networks — this machine's among them — where it is only ever a miss.

## Deezer as a streaming link

`Reference::streamed_at` is `deezer::streamed`, for a share the catalog holds no link for:
`/track/isrc:<code>` where the `StreamAsked` carries an ISRC (exact), and otherwise — or where
Deezer holds nothing under the code, answering the unknown-artist `DataException` — `/search` on
artist and title run together, capped at `SEARCHED_AT_MOST` (10). A hit is taken only where its
`title` or `title_short` folds to the asked title and its artist to the asked artist — both through
`folded_letters` with all but letters and digits dropped — and its length is within
`LENGTHS_AGREE_WITHIN` (3 s) where both are known, so an edit or live take is passed over. Only a
link on `https://www.deezer.com/` is answered, as `Relation::Streaming` under `Service::Deezer`.
Deezer's own field syntax (`artist:"…" track:"…"`) answers an empty list for every query now, hence
the plain words. `deezer_track_isrc.json` and `deezer_search.json` are the fixtures, and the live
test asks both routes.

## ListenBrainz

- **What was heard is posted as JSON under the listener's token, and nothing else about them.**
  `ListenBrainz` is the `Scrobbler` the library's `submit_listens` is handed. It POSTs
  `/1/submit-listens` on `Host::ListenBrainz`, paced at `LISTENBRAINZ_INTERVAL` (1 s), with
  `Authorization: Token <token>` — `Posted::authorization`, the one header a body carries that others
  leave `None` — and a `listen_type` of `single` for one listen and `import` for several (the service's
  name for a batch). A listen is its moment in whole seconds — when it *began*, `listens.began`, which
  `Library::track_played` writes as the counted moment less what had been heard of the visit, and the
  counted moment for a listen kept before the column — the artist, the title and, where held, the
  album, the recording, release, release group and artist MBIDs, the ISRC, the track number and the
  length in milliseconds (`Billed::release_group` is the album's `release_group` and `Billed::isrc`
  the track's `isrc`, both read by `billed_columns`),
  with `media_player`, `submission_client` and `submission_client_version` naming this build as its
  User-Agent does. An answer whose `status` is not `ok` is `Unreadable`; a status is `Refused` as
  every host's, which is how the library tells a malformed batch (400) from a refused token (401) and
  how the binary knows to stop asking under that token.
  `a_listen_says_what_was_heard_when_and_nothing_the_catalog_does_not_hold` pins the document;
  `listenbrainz_refuses_a_token_it_never_issued_and_says_so_by_its_status` is the live test proving the
  request reaches the service and is read there. **What is playing is told as it starts.**
  `Scrobbler::playing_now` takes a `Billed` — the track as a listen names it, which a `Scrobble`
  carries beside its id and moment — and `ListenBrainz` posts it as a `playing_now` listen with no
  `listened_at`, which the service shows and never counts
  (`what_is_playing_now_is_told_with_no_moment_and_as_one_listen`).
- **A favourite is a love, and a token is asked who holds it.** `Scrobbler::love` posts
  `/1/feedback/recording-feedback` with the recording's MBID and a `score` of 1, or 0 for
  `Love::TakenBack` — no feedback, not a hate (`a_favourite_is_told_as_a_love_and_taken_back_as_no_feedback_at_all`);
  a track with no recording MBID has nothing the endpoint takes and is never told.
  `Library::tell_loves` weighs the recordings of favourite tracks against `loves_told` — what the
  service was last told, per `ListeningService` — and tells the difference either way, at most
  `LOVES_TOLD_AT_ONCE` (25) a call so the submitter's two-second look is not held a minute behind a
  first run; a love refused as malformed is noted as told and not sent again
  (`a_favourite_with_a_recording_is_told_as_a_love_once_and_taken_back_when_unmarked`). Unlike a
  listen, a favourite is a state, so every favourite held when a token is first given is told.
  `Scrobbler::token_held` asks `GET /1/validate-token` under the same `Authorization` —
  `Client::json_as`, the one GET carrying one, `Sending` being what `exchange` is handed — and answers
  `TokenHeld::By` the user the service names or `Unknown` where it says `valid: false` or refuses the
  token outright (`a_token_is_held_by_the_user_the_service_names_and_by_nobody_it_calls_invalid`,
  and `listenbrainz_says_a_token_it_never_issued_is_held_by_nobody` live). `LookupOp::Love` and
  `LookupOp::Token` name the two.
- **The binary's half is a thread following the file.** `submitting.rs` starts `resonate-submit` for
  the window and every playing command: five seconds after start and every `SUBMITTED_EVERY` (30 s)
  after, it asks `Library::submit_listens`, doubling the wait after each failure up to an hour.
  Between those it looks at the player every `PLAYING_LOOKED_AT_EVERY` (2 s), and a row begun since
  the last look is read through `Library::billed_as` and told as playing now — once per row, not
  while paused, never for a file the catalog names nothing for. Its `Token` follows the file as
  `Followed` does: the modification time is weighed and `config::submitting_in` reads `online` and
  `listenbrainz-token` only where it moved, so a token typed into the window's *ListenBrainz* group
  or written by hand is carried by the next submission, and `online` turned off stops it. The
  `ListenBrainz` client is built once per token and kept across the looks, so its connections and its
  pacing outlive the two seconds between them; a token that moves builds the next. A 401 or
  403 holds that token back until the file names another. A build without the feature starts
  nothing and warns once where a token is set. Each submission is followed by `tell_loves` under
  the same scrobbler, and a 401 or 403 to either holds the token back.

## LRCLIB

- **`Lrclib` reads what the catalog kept before it asks, and keeps what it is told.** A
  `LyricProvider` named `lrclib`, refusing a `Wanted` naming no title or artist (the service has
  nothing else to search on). `Library::kept_lyrics` is read first, keyed by the `Wanted`'s location
  and `Wanted::span`: a kept row not `KeptLyrics::is_due` answers without a request — its words, or
  nothing for a kept miss — and a due one is asked again, the richer of kept set and answer going to
  the pane, a failed request falling back to what was kept. A provider built with no library keeps
  nothing and asks every time.
- **The window and the lookup ask one way.** `lrclib::told` is the whole question — a `LyricsAsked`
  in, an `Option<LyricText>` out — and both `Lrclib` and `Online`'s `Reference::lyrics` go through
  it, the first mapping a `Failed` into a lyric error under the `LyricOp` it failed at, the second
  into the library's error under `LookupOp::Lyrics`.
- **`/get` is exact, `/search` weighed.** `ask` asks `/get` with track name, artist name and, where
  known, album name and duration in whole seconds; a 404 falls through to `/search` on title and
  artist, and `pick` weighs every answer that `names` the track — title and artist each folded to
  lowercase alphanumerics and compared for equality — and `lasts_about` as long, within
  `LENGTH_MAY_DIFFER_BY` (30 s, the tolerance `Sidecar` gives a `[length:]`), and takes the best by
  `Worded` — synced, then plain words, then wordless, then instrumental — and among equals the
  nearest in length, a hit of unknown length after every known one and the first of true equals
  (`a_synced_hit_is_taken_over_an_earlier_plain_one_and_the_nearest_in_length_among_equals`). An answer flagged
  `instrumental`, or carrying no words, is `None`, kept as a miss. Otherwise a `LyricText` of
  `syncedLyrics` where present and `plainLyrics` where not, with the answer's `lyricsfile` beside it
  **only where that document says more than its lines**: LRCLIB writes a Lyricsfile for every record,
  and one written from an LRC ends each line where the next begins and times no word — the LRC again
  at four times the size. A document timing a word, or ending a line anywhere but where the next
  begins, is kept. A failure on `/get` is a lyric error under `LyricOp::Fetch`, on `/search` under
  `LyricOp::Search`.
- **A Lyricsfile is read by `resonate-lyrics`, and LRCLIB's answer goes through it.**
  `read_lyricsfile` lives beside the LRC reader, so a `.lyricsfile.yaml` sidecar is read by the same
  code (`lyrics.md` has the sidecar's side): `serde-saphyr` with duplicate keys refused, one document,
  `MOST_NODES` and a depth of `DEEPEST`, the text capped at `LARGEST_LYRICSFILE` before parsing and
  `MOST_LINES` and `MOST_WORDS` after. Only version `1.0` is read (the draft says an unknown version
  must not be read as it), and `offset_ms` is ignored, the draft not saying which way it runs. A
  line's `words` become `SungWord`s *laid over* the line's text, each found in order and handed the
  text up to the next, so a writer who left the spaces off still draws the line as written; where a
  word cannot be found the words are joined as they are. A line's `end_ms`, or its last word's, is its
  declared end. **Two voices are read off the timing**: a line starting before the voice-one line in
  play has ended is voice two, the Lyricsfile having no word for a singer and overlapping lines being
  how it writes two. An instrumental answers nothing, a document timing no line is read as its
  `plain`, and a kept document is read back through the same reader, one no longer reading falling
  back to the kept LRC text.

## Recognising a clip

- **Two services behind `resonate-listen`'s `Recogniser`, asked in order.** `online::recognisers`
  registers `Shazam` wherever `online` is on and `Audd` where `audd-token` holds one — not AcoustID,
  which cannot match a clip (`analysis.md`); `Recognisers::recognise` takes the first that names the song, and a silent clip
  is sent nowhere. A refusal is a `tracing` record and a name in `Recognition::refused`, not the end
  of the walk.
- **The client POSTs as well as GETs, through one exchange.** `Client::exchange` is the loop that
  paces, sends and asks a 503 again, with a `Posted` body — content type, whether `Encoded::Gzip`, the
  bytes — or none; `Client::posted` reads the answer under `LARGEST_DOCUMENT` as `json` does. A POST
  carries `Content-Language: en_US` beside the User-Agent, and `Content-Encoding: gzip` where packed,
  which only `Posted::packed_form` makes.
- **Shazam is asked with a signature, never audio.** `Shazam::recognise` takes `resonate-analysis`'s
  `signature_of` over the clip's mono mix and POSTs it to
  `/discovery/v5/en/US/android/-/tag/<uuid>/<uuid>` with the query flags the endpoint expects: the two
  ids fresh `uuid` v4s per request, the location all zeros, the timezone UTC, the user agent this
  build's, so nothing about the listener goes out but the peaks of twelve seconds of sound. An empty
  `matches` is nothing heard; a match is read for `track.title`, `subtitle` as artist, the `SONG`
  section's *Album* and *Released*, the ISRC and the track's page, and its `coverarthq` is fetched from
  `AppleArtwork` — only an https URL whose host ends in `.mzstatic.com` — and handed on as a `Picture`
  sniffed by its first bytes. The endpoint is undocumented and the likeliest to change;
  `shazam_answers_a_clip_it_does_not_know_with_nothing_rather_than_a_refusal` is the live test that
  notices.
- **AudD is sent the clip itself, only with a token the listener typed.** `Audd::recognise` writes
  the mono mix as a 16-bit WAVE and POSTs it as `multipart/form-data` — the token,
  `return=apple_music,musicbrainz` and the file — under a boundary minted per request, refusing a clip
  past the service's 10 MB. A `status` other than `success` is `Refused` with the service's error
  code; a `null` result is nothing heard. It reads title, artist, album, the year of `release_date`,
  the ISRC off Apple Music or MusicBrainz, the first MusicBrainz recording and the song link, and
  fetches the Apple Music artwork at 600 px through `AppleArtwork`.
- **A file nothing else can name is named by ear, where the listener said so.** `ByEar` is a
  `Fingerprints` the binary registers after `AcoustId` wherever `online` is on, answering only while
  the shared `identify-by-sound` switch is on — `Fingerprints::answers`, which
  `Fingerprinters::has_a_source` and `recognise` both weigh, so a switch off costs no study and asks
  nothing. It reads twelve seconds through `resonate_analysis::excerpt` — from a third of the way in
  or thirty seconds, whichever is sooner, mixed to mono — signs them with `Shazam::signed` (Listen's
  request), and turns what Shazam names into recordings: its ISRC through `recordings_of_isrc`, the
  takes within `LENGTHS_AGREE_WITHIN` of the file's length scored whole, otherwise a `find_recording`
  phrase search of title, artist and album scored as MusicBrainz scored it. The pass weighs them as
  any recognition — the strict score, `Certainty::Nearly`, so a name the file never gave is filled and
  none it gave touched — which is what finally names a file with no title, no artist and a stem saying
  nothing.

## Fixtures and the live test

**The mapping is proved over captured answers; the services are reached only on request.**
`tests/fixtures/` holds captured documents per shape — `release.json`, `release_linked.json`,
`release_search.json`, `recording.json`, `isrc.json`, `recording_search.json`, `release_group.json`,
`release_group_search.json`, `release_group_browse.json`, `artist.json`, `artist_search.json`,
`coverart.json`, `coverart_group.json`, `wikidata.json`, `wikipedia.json`, `lrclib_get.json`,
`lrclib_search.json`, `acoustid_lookup.json`, the Deezer answers (`deezer_artist.json`,
`deezer_unpictured.json`, `deezer_no_data.json`, `deezer_track_isrc.json`, `deezer_search.json`),
`autoeq_index.md`, `autoeq_parametric.txt`, `shazam_match.json` and `shazam_nothing.json` (captured live
with a signature of a library track and of synthetic notes), and `audd_recognised.json`,
`audd_nothing.json` and `audd_refused.json` (written from the service's documentation, no token being to
hand); a worded Lyricsfile, `resonate-lyrics`'s `lyricsfile_worded.yaml`, is shared with `lrclib.rs`.
Each is pulled into its module's tests with `include_str!`, so the walk from a document to a `Release`,
`Recording`, `ReleaseGroup`, page of `ArtistRelease`s, `ArtistProfile`, front URL, `Told`, `Catalogue`
or `Profile` runs on every `cargo test` with no network. Captured whole, not cut down, which lets a test
assert over all twenty-five releases a recording names. `tests/live.rs` is the other half: gated on
`RESONATE_ONLINE_TESTS` and printing a skip without it, it shares one client through a `OnceLock` so the
pacing holds across its tests, and asks the real services for the release, artist and track the fixtures
were captured from, that artist's release groups (at least a hundred, *Meddle* among them), AutoEq's
whole index and one correction, Deezer's two stream routes, LRCLIB, Spotify and SoundCloud pages, Shazam
and ListenBrainz.

## The binary's half

**`crates/resonate/src/online.rs` is the only file naming `resonate_online`'s types, and it has a
twin for the build without it.** Under the feature, `reference` answers an `Online` over a fresh
`Client` only where `Config::online_enabled`; `corrections` is `Corrected::uncorrected()` with an
`AutoEq` appended under the same condition; `lyricists` is `Lyricists::local()` with an `Lrclib`
appended through `Lyricists::and`; the last two are handed the library where there is one, so the
catalog keeps what each fetched; `fingerprinters` and `recognisers` register the printers and
recognisers above. `reference_asked_for` and `corrections_asked_for` are the same two or
`Error::OnlineOff`, the second asking `Corrected::has_a_source` rather than the key again, since a
build with the feature off has a `Corrected` answering nothing. Without the feature the twins answer
`None`, `Error::NoReference`, `Corrected::uncorrected()`, `Error::NoReference`, `Lyricists::local()`
and the stubs, so `main.rs` reads the same names either way — `lyricists` compiled only into a `ui`
build in both halves (nothing headless draws words), and `corrections` `ui`-gated in the featureless
twin alone. Other files gate on the feature too — `providers.rs`, `submitting.rs`, `config.rs`, the
two error variants and `Config::online_enabled` — but none names the crate's types. Every one of
them — the reference, correction source, lyric provider, printers, recognisers and ListenBrainz — is
built over the one `Arc<Client>` in `online::CLIENT`, made on first use over `INTRODUCTION`, so
the process keeps one agent and its connections; the pacing would be shared across clients anyway.
