# YouTube Music provider

Status: approved by the listener on 2026-11-06. Decisions are marked **Listener** (theirs) or **Builder's
call** (made while writing this, open to correction).

## Intent

The listener pays for YouTube Premium and wants songs from YouTube Music kept on their own machine
for offline listening, at the best quality the account allows. Some of those songs exist on no other
service Resonate can reach, so this has to work for a song Resonate has never heard of, not only for
a song MusicBrainz names.

They want to connect the account once and then have Resonate do the work from their playlists and
from search, rather than running a tool by hand per song.

## Goals

1. Connect a YouTube Premium account to Resonate once, from settings.
2. Answer wants: a song the library is missing is fetched from YouTube Music where nothing better
   has it.
3. Search YouTube Music from the window and keep a song found there.
4. See the account's own playlists and liked songs, and keep a whole playlist.
5. Get the highest tier the account offers: Opus about 260 kbps (itag 774), else AAC about 258 kbps
   (itag 141), else Opus about 134 kbps (itag 251).
6. Never re-encode or remux (**Listener**). What lands is the stream YouTube served, byte for byte,
   but for tags written into the file.

## Non-goals

- Android and iOS (**Listener**: desktop first). `yt-dlp` cannot run there.
- Flatpak. The sandbox holds no `yt-dlp`, so a Flatpak build shows the provider as unavailable.
- Video. Audio only.
- Writing to the account: no liking, no playlist edits.
- Keeping playlists in sync on a timer. A keep is something the listener presses, once per press.

## Anti-goals

- **Quietly worse files.** A library filling with 160 kbps files because cookies lapsed is a failure
  even though every download "worked".
- **Touching the account unasked.** Nothing runs until the listener turns it on.
- **A re-encode or remux anywhere**, including a helpful default in `yt-dlp`.
- **Taking a lossless song's place.** YouTube fills gaps. It answers a want only after every better
  source has said no (**Listener**). The one exception is a song the listener pressed Keep on.

## Constraints

- Saving files this way breaks YouTube's terms even on a paid account, and it runs on the account's
  own cookies. The listener has been told and chose to go on. The settings group says so in one
  plain sentence beside the switch.
- A provider crate reaches the seam, core and, over the network, `resonate-fetch` alone
  (`CLAUDE.md`). This one uses `resonate-fetch` for search and `yt-dlp` for the rest.
- `online = false` stops everything. A metered connection holds downloads as it does for any
  provider (`providers.md`).
- The repo's rules apply: no comments, `thiserror` per `errors.md`, `rust-formatter`, full version
  pins, every stored format through `MIGRATIONS` if the schema moves.
- **The tag writer does not write Matroska.** `FileTags::writes` passes `.mka` over (`library.md`) and
  `filed::tagged` then lands the file untagged. A kept song has no release row behind it, so its
  names would be lost on the next scan. With no remux, the writer has to learn Matroska (see
  *Tags in Matroska*).

## Engine

**Listener:** `yt-dlp`, run as an external program, found on `PATH` or at a configured path. It is a
separate install. `ffmpeg` is not needed.

Why a sidecar rather than a Rust client (RustyPipe): YouTube's player changes often, a sidecar is
fixed by updating it rather than by releasing Resonate, and Premium formats are not proven through
RustyPipe.

`yt-dlp` does what needs the account or the player's JS: formats, private playlists, the download.
Searching does not go through it (see *What was measured*).

## What was measured

On 2026-11-06, `yt-dlp` 2026.10.26:

- **Premium audio is reachable.** On the listener's account, through Zen's profile, an album song
  listed `774` (Opus 261k) and `141` (AAC 258k) with `player_client=web_music`, beside `1080p Premium`
  video rows that show the cookies counted.
- **A music video has none.** The same account on a music video listed `251` (134k) and `140` at
  best. So a kept video lands at the lower tier by nature, and the song *Check connection* and the
  re-check use must be an album song.
- **The JS challenge is solved by `deno`** without being asked. `node` was not needed.
- **`yt-dlp`'s *Songs* search names only id and title**: no length, artist or album, so
  `named_alike` could not pass. This tripped the gate below.
- **A public InnerTube search has everything.** `POST music.youtube.com/youtubei/v1/search` with
  client `WEB_REMIX` and the Songs filter answered 20 rows, each with a video id, title, artist, album,
  length and a `MUSIC_EXPLICIT_BADGE` where explicit. No cookies, so a search never touches the
  account.

- **A private playlist's flat listing names the song.** `LM` through the listener's cookies gave `id`,
  `title`, `duration`, `channel` and `uploader` for each row, with no album. The channel is the artist:
  `Кошка - Topic` on an album song, a plain `noone` on an upload. So a kept playlist needs no call per
  song, and the ` - Topic` suffix is cut.
- **A real `774` download.** `9RfVp-GhKfs` (*Creep*, from the Songs filter) landed as a 7.8 MB `.mka`:
  Opus, 48 kHz, stereo, 263 kbps, 238.7 s. Its DocType is `webm`. The Segment has a known size in an
  8-byte field. Top level: `SeekHead` (Info, Tracks, Cues), `Info`, `Tracks`, `Cues`, 24 `Cluster`s. There
  is no `Tags` and no `Void`, and the `Cues` sit ahead of the clusters, so an element added ahead of them
  shifts every cue position.

Not yet measured: the AAC path.

## Shape

**Builder's call:** one crate, `crates/providers/ytmusic`, package `resonate-ytmusic`, source and
scheme `ytmusic`, behind the binary's `online` feature. Depends on `resonate-core`,
`resonate-fetch`, `resonate-providers`, `serde`, `serde_json`, `tracing`, `ureq`.

It implements two seams:

- `Provider`, so wants are answered.
- `PlaysInPlace`, so the window can search, browse and play.

Everything that runs `yt-dlp` goes through one small type in the crate (call it `Tool`) holding the
binary's path, the cookie source and the JS runtime. Tests point it at a fake executable.

### Connecting the account

**Builder's call:** settings keys, all empty or off by default, read by `Accounts` in
`crates/resonate/src/providers.rs` beside the others:

| Key | Meaning |
|---|---|
| `youtube-music` | Bool, default false. Nothing is registered while off. |
| `youtube-music-cookies` | A browser name (`firefox`, `chromium`, ...), a browser and profile folder as `yt-dlp` spells them (`firefox:/path/to/profile`, which is how a fork such as Zen is named), or a path to a Netscape cookies file. A value starting with a browser name and a colon is a browser; any other value holding a path separator is a file. |
| `yt-dlp` | Optional path to the program. Blank means `PATH`. |

A settings group `Group::YoutubeMusic` in the Library category holds these, the terms sentence, and
a *Check connection* button. The check runs a format listing on one fixed song and says one of:
Premium formats available; signed in but no Premium formats; not signed in; `yt-dlp` missing. The cookie value is withheld from every `Debug`, as other secrets are.

The provider works with no cookies at all, at the free tier, once `youtube-music` is on.

Every call names the song as `https://music.youtube.com/watch?v=<id>`: `yt-dlp` adds its `web_music`
client for a `music.youtube.com` address by itself (`_video.py`). `yt-dlp` finds `deno` itself; `--js-runtimes` is passed only where `deno` is absent, naming the first
of `node`, `bun` found on `PATH`.

### Answering a want (`Provider::find`)

1. If the identity carries a YouTube link (`Identity::track_on`), use that video and skip the search.
   A keep is pinned to its video, and may have no length or artist.
2. With no link, ask `Identity::may_be_listed`, since it guards a search by name.
3. Otherwise for each of `Identity::wordings`, search YouTube Music's Songs filter over InnerTube,
   signed out, at most 25 listings, and hand each to `Choosing` as a `Listed`. The seam decides.
   YouTube gives no ISRC, so this is always the named-alike rule: lengths within 2 s, titles and lead
   artist equal. The client version is read from `/sw.js_data`, not pinned: under `resonate/<version>` the home page
   redirects to consent and then a "browser out of date" page with no version in it.
4. Only songs. Uploaded music videos are not taken, since a video's audio is often a different cut.
5. Probe the chosen listing for its formats. If no wanted format exists, answer `Nothing`.
6. Answer `Obtained::Found(Delivery::Stream { key: "track/<videoId>", extension, opening })`.
   Finding is not downloading: the `Opening` starts the download only when opened.

**Listener: strict last resort.** YouTube answers a want only once every earlier provider has been
heard and offered nothing. Asking it last is not enough: `Providers::first` takes a later offer once
`grace` has passed while an earlier provider is still asked, and passes over one that is away, and a
want once held is never due again, so a lossy file would stand in for a FLAC for good. The seam gains
a way for a provider to say it is a last resort (name left to the builder): `Providers::first` holds
its offer until every earlier turn is over, and takes nothing from it where an earlier provider was
late, refused or away, so the want stays due for the next poll. The check is
`Answer::heard_from_every_provider`'s, applied to the providers ahead of this one. A keep is
exempt, being pinned (below).

**Builder's call:** the provider does not declare `downloads_at_once`, because that is the narrowest
across the whole registry (`supply.rs`) and would drop every other provider to one lane. It holds one
download at a time itself, behind a lock inside the crate. Searches are paced by a `Pacing`, 2 s
apart. Registered last in `providers::registry`, after Monochrome.

### Rating

`Listed::stated` is `Rating::flagged` of the row's `MUSIC_EXPLICIT_BADGE`, so explicit-first works as
it does for TIDAL. A clean take with an unmarked title can still be taken, as on any service, unless
`Rating::of` reads a qualifier.

### Getting the bytes

An `Opening` runs `yt-dlp -f <format id> -o - <url>` and hands its stdout to the poll untouched.

- **Opus (774, 251):** extension `mka`. WebM is Matroska, and `mka` is already in `AUDIO_EXTENSIONS`,
  the MIME list and the desktop entry, so nothing about what is offered changes. **Builder's call:**
  the name changes, the bytes do not. The decoder must be shown to take a `webm` DocType under that
  name.
- **AAC (141):** extension `m4a`, as served. If the tag writer refuses DASH's fragmented MP4 it lands
  untagged and the song's names stay in the catalog, and it is noted in `providers.md`. It is only
  reached where 774 is absent.

The child is killed when the reader drops. A non-zero exit is an `Err` from `read`, never an end of
file, so a cut download cannot look whole. Stderr goes to `tracing` at debug. An opening is whole,
not resumable. `length` is the probe's stated `filesize`, where it stated one.

Existing checks still apply after delivery: the decoder must probe it and
`Want::lasts_as_long_as` must agree.

### Quality and failure

- Selector order 774, 141, 251. The tier taken is logged at info with the song.
- **Lapsed cookies stop the poll.** When cookies are configured and `yt-dlp` says `The provided
  YouTube account cookies are no longer valid` (`extractor/youtube/_base.py`),
  the provider answers a provider-away error, so the poll notes it once, stops asking and the want
  stays due. It does not fall back to the free tier for that run.
- **Signed in but no Premium format on a song:** before taking a lower tier, once per poll, the
  provider re-checks the fixed song *Check connection* uses. If that song shows no Premium formats
  either, the account has lost them (a cancelled subscription, a `yt-dlp` client change) and the
  provider answers provider-away for the poll. If it still has them, this one song lacks them: taken at the
  tier it has, logged at warn. (**Builder's call.**)
- `yt-dlp` missing: `Error::Io`, already provider-away.
- **Builder's call:** a new seam variant `Error::ToolFailed { provider, op, code: Option<i32> }`,
  provider-away, for a non-zero exit that is neither of the below. A deleted, private or
  region-blocked video answers `Opened::Gone`.
- Every `yt-dlp` call has a time limit and is killed past it, since a hung child must not hold a
  poll lane.

### Search, playlists and play (`PlaysInPlace`)

- `scheme` is `ytmusic`, `title` is "YouTube Music". Registered in `in_place` after Monochrome and
  added to `PLAYED_IN_PLACE`.
- `search` lists Songs over InnerTube (albums and artists are `Found` left empty in this version),
  then `Found::explicit_first`.
- `first_page` lists the account's own playlists and liked songs as `Found::mixes`. Without cookies
  it is empty. Measured: `music.youtube.com/library/playlists` is refused by `yt-dlp` (a redirect to a
  404); `www.youtube.com/feed/playlists` answers `LL`, `WL` and the listener's own lists. So `LM` is
  listed first as *Liked songs*, then that feed without `LL` and `WL`.
- `mix` lists a playlist's songs.
- `PlaysInPlace` gains `plays_songs` (default true), false here, so the window offers no *Play* on
  these rows. `streamed` is `NotOffered` under `ProviderOp::Stream`. **Builder's call:** no playing in place in
  this version. The listener asked to keep songs. A queued row carries only its location and is
  probed off the stream's head, so playing in place would show blank rows and would start a `yt-dlp`
  per probed row, on the account, for rows nobody pressed. A found song is kept, then played from
  disk.
- `artists` is empty; `artist` and `album` are `NotOffered`.
- `cover` and `cover_of` read the row's thumbnail URL over `resonate-fetch`.

### Tags in Matroska

**Builder's call, a consequence of no remux.** `FileTags` learns to write `.mka`, in `resonate-codec`
beside the existing heads: a `Tags` element holding the fields `tags.rs` already reads off Matroska,
replacing any held, with a `Void` element kept for the next edit. lofty has no Matroska, so this is
hand-written over `matroska.rs`. Clusters are never rewritten or re-timed: every audio packet stays
byte for byte, and a test holds that. Segment and `SeekHead` sizes are corrected where the element
moves them. It follows every rule of *Writing the catalog back into the files* (staged copy,
`Held`, read back and confirmed). `.mka` the listener already holds gains tag writing from this too.
Covers go in a Matroska attachment only if the reader already takes one; otherwise later work.

### Keeping a song found there

**This is new infrastructure, not a provider detail.** Nothing today keeps a served song:
`Library::want_found` takes a MusicBrainz find. Goals 3 and 4 need it.

**Builder's call:** add a *Keep* action on a YouTube Music song or mix in the served view, and
`resonate ytmusic keep <url>` on the command line.

**A keep is its own queue, not a want.** `wants.release_track_id` is `NOT NULL UNIQUE`: a want is a
release row, and a song MusicBrainz has never heard of has none. Inventing release rows would put them
in the Missing pane and under `land_release`. So a keep is a row of a new table `kept_songs` (a
`MIGRATIONS` step) holding the song's title, artist, album, length, video id, and where it belongs in
a kept playlist. A pass of its own walks that table one song at a time: it builds an `Identity`
carrying the video's `Link`, asks the `ytmusic` provider alone (`Providers::only`), and files the
delivery through the same `filed` path with the same length check. `Provider::find` sees the link and
takes that video directly (step 2 above), so no search is repeated and no MusicBrainz entry is needed.

**Listener: a keep is pinned to the video pressed.** Asking one provider alone is what pins it: TIDAL
or Monochrome are never asked, so they cannot answer with a same-named file.

A keep lands in the delivery folder (`music-folder`, `organise-as`). With none set it is refused with
a clear error. A vault is not a keep's destination in this version.

Tags on the landed file come from YouTube's metadata for that song. A guess is never written
(`CLAUDE.md`), so the existing enrichment may later improve a kept song by strict match only.

**A song already held is not kept again (Builder's call).** Before writing a want, a keep looks for
a held track by the same strict name rule as `Identity::named_alike`. A match writes no want, and a
playlist keep puts the held track in the list instead, so a FLAC is never shadowed by a lossy copy.
Pressing keep twice is therefore harmless.

**What a keep may take (Builder's call).** Step 4's "only songs" binds searching on a want's behalf.
A video the listener pressed, or that sits in a playlist they kept, is taken as it is, since those
may be the very songs no other service has. Step 2 follows a `Service::YoutubeMusic` link, and a
`Service::Youtube` link only on a want written by a keep: MusicBrainz's plain YouTube links are often
videos and go through the search like any other want.

A kept playlist is one want per song, filled into a local playlist of the same name through
`Library::fill_playlist_named`.

**Listener: YouTube's order by default, with an option for landing order.** Songs land over hours or
days, possibly in another process, so order needs remembering: `kept_songs` holds each song's playlist
and place, and a song that lands is placed among those already there, in any process. **Builder's calls:** a setting
`youtube-music-keep-order`, default true, shown in the group; false appends in landing order and
writes no places. The playlist appears as soon as its first song lands or was already held. A song
that gives up simply leaves a gap in the order, with its place kept. Pressing keep again adds songs
new to the YouTube playlist, in their places, and leaves rows the listener removed locally removed.

A public YouTube playlist link already reaches wants through `Library::follow_playlist_link`, for the
songs MusicBrainz can name. The rest are counted `unnamed` and dropped as they are today. **Builder's
call:** accepted in this version; the account's own playlists go through keep and do not lose them.

## Order of work

1. **Finish the spike, thrown away afterwards.** The gate is passed: 774 and 141 exist on the
   account, and search moved to InnerTube. What is left is under *What was measured*. The listener
   runs anything needing their cookies, since no agent should read their browser's cookies. A
   difference adapts this document without stopping.
2. Tags in Matroska, in `resonate-codec`, since nothing depends on YouTube and everything kept needs it.
3. Crate, `Tool`, `Provider::find`, delivery, with a fake `yt-dlp` in tests.
4. Settings, registration, *Check connection*, the strict last resort in the seam.
5. `PlaysInPlace`: search and playlists.
6. Keep, in the window and the CLI: pinning, the held check, the order table.
7. Rules: a section in `providers.md`, keys in `binary.md`, thirty-five crates and the tree in
   `CLAUDE.md`, the `layering` job, `optdepends` in `packaging/`.

## Testing

- A fake `yt-dlp` script in the crate's tests answers fixed JSON and fixed bytes: a chosen listing,
  no wanted format, lapsed cookies, a non-zero exit mid-stream, a hung child, a missing binary.
- A short real Opus-in-WebM fixture delivered as `.mka` decodes through `resonate-codec`, is tagged,
  reads its tags back, and its audio packets hash the same before and after, skipped without `ffmpeg`
  as the repo's other tests are.
- A live test behind `RESONATE_YTMUSIC_TESTS`, off in CI, run by hand on the listener's account.
- CI has no `yt-dlp` and needs none. The Windows cross-check still has to compile.

## Left to the builder

Names of types and tests, how `Tool` shapes its calls, the exact `Pacing` and time limits, and where
the *Keep* button sits.
