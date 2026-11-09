# Signing in without typing a secret: Jellyfin, Last.fm, YouTube Music

## Intent

Signing in to a service should be "press *Sign in*, approve somewhere you already trust, done",
not typing a password, pasting a token, or naming a browser profile. The listener asked for it
because it is convenient; nothing is broken today. Spotify (PKCE over a loopback redirect) and
TIDAL (device code) already work this way and are untouched.

Three independent pieces, each shippable on its own, in this order:

1. **Jellyfin Quick Connect**: desktop and both phones.
2. **Last.fm browser approval**: desktop and both phones.
3. **YouTube Music sign-in window**: desktop alone (Linux and Windows).

## Goals

- No password is typed into Resonate for Jellyfin (where the server allows Quick Connect) or for
  Last.fm (always).
- A YouTube Music account is reached by signing in to Google in a window Resonate opens, with
  "Signed in · Premium" (or not) shown straight after.
- The window, the command line and the phones agree: each piece is one seam in Rust, with a front
  end on each surface it reaches.

## Non-goals

- Shipping any client ID, API key or secret with Resonate. **Last.fm keeps the listener's own API
  key and secret** (`lastfm-key`, `lastfm-secret`); the "listener-set only" rule in `online.md`
  stands unchanged. The listener considered a built-in key and declined it.
- Spotify, TIDAL, ListenBrainz, Subsonic, AcoustID, AudD: no change. Subsonic and the key-only
  services have no such flow; ListenBrainz's token is not handed out by MetaBrainz OAuth as far as
  is known.
- YouTube Music on the phones. The provider is `yt-dlp`, which cannot run there, and Google refuses
  sign-in inside an Android WebView. Phones already receive songs the desktop kept, through sync.
- Google OAuth / the YouTube Data API: it needs a Google Cloud client and yields no audio, and
  `yt-dlp` dropped its own OAuth login in late 2024.

## Anti-goals

- A secret reaching a log, `Debug`, a crash report or a URL in the log (the existing `<withheld>`
  discipline extends to every new type: Quick Connect secret, Last.fm token, the cookie jar's path
  and contents).
- A browser engine linked into the GPUI window process, or into a build without the `online`
  feature.
- Losing an existing working setup: a listener with `jellyfin-password` or a browser named in
  `youtube-music-cookies` keeps working with no action.

## 1. Jellyfin: Quick Connect

**What the listener sees.** Beside the server address on the Jellyfin account pane, *Sign in with
Quick Connect*. Pressed, it shows a six-character code and "Approve this code in a Jellyfin app you
are signed in to (Settings → Quick Connect)", with a Cancel. Once approved, the pane shows the user
signed in as, and no password field is needed. The user and password fields stay, as the other way
in: a server may have Quick Connect off, and Emby has none. Where the server answers that Quick
Connect is off, the pane says so and offers the password.

**The flow** (Jellyfin 10.8+): `GET QuickConnect/Enabled`; `POST QuickConnect/Initiate` (with the
existing `MediaBrowser` authorization header) answers `Code` and `Secret`; poll
`GET QuickConnect/Connect?secret=…` until `Authenticated`; then
`POST Users/AuthenticateWithQuickConnect` with `{"Secret"}` answers `AccessToken` and `User.Id`,
the same `Session` the password sign-in produces. A code lapses server-side after a few minutes; a
lapse, a cancel and a refusal end with no token, as TIDAL's device flow does.

**What is kept.** There is no password to sign in again with, so the session itself must persist:
the access token, user id and the device id it was issued to (Jellyfin ties a token to its
`DeviceId`, so the per-instance random UUID becomes a kept one for this case). New settings beside
the existing `jellyfin`, `jellyfin-user`, `jellyfin-password`. Every new value prints `<withheld>`.

**When the kept token is refused** (401 after a sign-out on the server, a revoked device): the
provider is away under `ProviderOp::SignIn` as today, and the pane offers Quick Connect again.

**A token and a password never both stand** (author's call, not the listener's): a Quick Connect
sign-in clears `jellyfin-password`, and a password typed afterwards clears the kept token, so a
refused token has no password to fall back to and is simply away. A password sign-in behaves
exactly as today (password kept, a session per run, a random device id per instance, 401 signs in
again once); only the Quick Connect case keeps a token and device id. The account is ready, on the
desktop (`Accounts`, `providers::in_place`) and on the phones (`Readiness`, `is_ready`), with an
address and either user and password, or a kept token; the phones' stored account records gain the
token, user id and device id as sealed fields.

**Surfaces.** The window's Jellyfin pane; the command line (a Jellyfin sign-in that prints the code
and waits, first interrupt cancels, as `resonate tidal` does); Android's provider account screen
(`ProviderAccountScreen.kt`, `ProviderAccountStore.kt`); iOS's equivalent. The flow lives once in
`resonate-jellyfin`; the phones reach it through `resonate-mobile`.

**Emby** has no Quick Connect: the button is not offered where the server reads as Emby (`Flavour`).

## 2. Last.fm: browser approval

**What the listener sees.** With an API key and secret set, *Sign in to Last.fm* opens the system
browser on Last.fm's "Allow access?" page. The listener presses Allow; Resonate notices on its own
and shows "Signed in as <name>". No user name or password field remains.

**The flow** (Last.fm's desktop authentication): signed `auth.getToken` → open
`https://www.last.fm/api/auth/?api_key=<key>&token=<token>` → poll signed `auth.getSession` with the
token; error 14 (not yet authorised) means keep waiting, a session answer means done. The token
lasts 60 minutes; Resonate stops waiting well before that (same lapse as Spotify's,
`A_SIGN_IN_LAPSES_AFTER`, 10 minutes) and on cancel. The session key is stored in `lastfm-session`
exactly as today; nothing else about the account is kept.

**The password path is removed** (`auth.getMobileSession`, `input::a_line_unechoed` for it,
`LastfmSignIn`'s password, `resonate lastfm --user`). The browser flow does not need a browser on the
same machine: `resonate lastfm` prints the address, tries to open it, and waits, so it works over SSH
too. `--forget` stays.

**Surfaces.** The window's scrobbling settings; `resonate lastfm`; Android's `ScrobblingScreen.kt`
and the iOS settings view, opening the page in a Custom Tab / `ASWebAuthenticationSession` (or the
system browser) and polling from Rust through `resonate-mobile`. No callback URL is involved, since
the listener's own API account decides that and polling needs none.

## 3. YouTube Music: a sign-in window

### What the spike showed (2026-11-08)

A throwaway Python + WebKitGTK 6 window on the listener's Hyprland session, loading
`accounts.google.com/ServiceLogin?service=youtube&continue=https://music.youtube.com/` with
WebKitGTK's default Safari-like user agent:

- Google let the sign-in through (one intermediate Google prompt page), no "this browser may not be
  secure".
- The cookies (`.youtube.com` and `.google.com`, HttpOnly included: `LOGIN_INFO`, `SAPISID`,
  `__Secure-3PSID`, …) written as a Netscape `cookies.txt` were read by `yt-dlp 2026.10.26` with
  `--ignore-config --cookies`: *Liked songs* (`LM`) listed, and a song offered formats 774 and 141,
  the Premium tiers.
- Not shown: how long such a jar lasts, and whether wry's WebKitGTK 4.1 (GTK 3) and WebView2 behave
  the same. Google may tighten the check at any time, which is why the browser choice stays.

### What the listener sees

On the Accounts settings, YouTube Music's main action becomes *Sign in to YouTube Music*: a window
opens on Google's sign-in, the listener signs in, the window closes on its own once
`music.youtube.com` is reached signed in, and the pane runs the existing check
(`ConnectionState`: `Premium`, `SignedInWithoutPremium`, …) and shows the answer. *Sign out* removes
what the window kept. The existing way, naming a browser or a `cookies.txt` (`youtube-music-cookies`),
stays below it as the other way in, unchanged. A lapsed sign-in (`SignInLapsed`) offers *Sign in
again*, which opens the same window.

### How it is built (decided)

- **A separate program owns the browser engine: `resonate-signin`** (the listener's decision). It is
  its own executable, the only one linking `wry` (WebKitGTK on Linux, WebView2 on Windows); the
  `resonate` binary, the window included, never links a browser engine, so no desktop package gains
  WebKitGTK as a hard dependency. The window starts it as a child process and waits for it to end;
  `resonate ytmusic sign-in` does the same from the command line.
- **It is optional in every package.** Arch `optdepends`, Fedora a subpackage the main one only
  recommends, Debian `Recommends`; the Windows archive and installer carry it. The Flatpak, having
  no `yt-dlp`, does not. Where `resonate-signin` is not found, the pane says to install it and the
  browser / `cookies.txt` way below stays usable; `resonate ytmusic sign-in` refuses with a typed
  error naming it.
- **The jar is a file of Resonate's own**, in its data directory, owner-only (`0600` on Unix),
  written whole and then put in place, never half-written. Signing in points `youtube-music-cookies`
  at it; signing out deletes it and clears the setting. A listener who had named a browser and then
  signs in through the window is switched to the jar (the browser value is not kept).
- **`yt-dlp` keeps refreshing the jar**: with `--cookies <file>` it writes back rotated cookies, which
  is wanted; a jar never browsed in elsewhere is what `yt-dlp` itself recommends, as a browser in
  daily use rotates them away.
- **The window keeps its own browser profile** (also in Resonate's data directory, never the
  listener's browsers), so *Sign in again* after a lapse may need no typing. Sign out wipes it.
- **Done is detected, not pressed**: the window closes once it is on `music.youtube.com` and the
  jar holds the YouTube sign-in cookies (`LOGIN_INFO` and `SAPISID` under `.youtube.com`). Closing the
  window first is a cancel and keeps nothing.
- Every log line about it withholds the jar's path and contents, as `Cookies::withheld` does today.

### Left to the builder

- `resonate-signin` is a new crate (the 36th): `CLAUDE.md`'s tree, the `layering` job (nothing
  depends on it; it may not reach the engine or the UI) and `dependencies.md` are updated with it.
  How `resonate` finds it (beside its own executable first, then `PATH`) and how the two talk (the
  jar's destination passed in, the outcome as the exit status) are the builder's.
- The exact wry version and how cookies are read off it (wry's cookie API, HttpOnly included, is
  expected; verify on both platforms).

### Where wry falls short (author's call)

The plan opens with a spike per platform: wry on WebKitGTK 4.1 and on WebView2, each signing in
past Google's check and handing over the HttpOnly cookies. A platform where it fails does not get
the window: the browser / `cookies.txt` way stays its only way in, and the pane does not offer
*Sign in to YouTube Music* there. No engine is bound directly and no third `unsafe_code` exception
is added for this.
- Copy and layout within the existing panes, following `ui.md`; error variants following
  `errors.md`.

## For all three

- Rules files gain what landed: `providers.md` (Jellyfin, YouTube Music), `online.md` (Last.fm),
  `android.md` / `ios.md` / `mobile.md` for the phones' surfaces, `binary.md` for the commands.
- Tests in the style already there: loopback fake servers for Jellyfin's Quick Connect and
  Last.fm's token/session exchange (approved, refused, lapsed, cancelled); the YouTube Music window
  itself is checked by hand, its jar writing, sign-out and the switch from a browser value by tests.
- No stored format's `V1` is edited without checking whether 0.1.0 has shipped (`CLAUDE.md`); new
  settings keys are additions.
