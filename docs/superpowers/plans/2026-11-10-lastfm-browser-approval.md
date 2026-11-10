# Last.fm Browser Approval Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Sign in to Last.fm by allowing Resonate on Last.fm's own page, never by typing a password into Resonate, on the window, the terminal and both phones.

**Architecture:** `resonate-online` gains Last.fm's desktop authentication (a signed `auth.getToken`, the page `https://www.last.fm/api/auth/?api_key=&token=`, a signed `auth.getSession` polled until it answers a session, error 14 meaning not yet) and loses `auth.getMobileSession`. The library's `Scrobblers` seam answers an approval (`LastfmApproval`: the address and a wait), as Spotify's does; the terminal prints and opens the address, the window opens it and waits with *Stop*, and the phones get `authorize_lastfm` / `finish_lastfm_authorization` / `cancel_lastfm_authorization`.

**Spec:** `docs/superpowers/specs/2026-11-08-sign-in-design.md`, section 2. Code map: `.superpowers/sdd/map-jellyfin-lastfm.md`.

## Global Constraints

- The listener's own API key and secret stay (`lastfm-key`, `lastfm-secret`); the session lands in `lastfm-session` as today; nothing else about the account is kept.
- The wait ends after `A_SIGN_IN_LAPSES_AFTER` (10 minutes, Spotify's) and on a cancel, keeping nothing.
- The password path goes: `auth.getMobileSession`, `input::a_line_unechoed`, `LastfmSignIn`'s user and password, `resonate lastfm --user`. `--forget` stays.
- `resonate lastfm` prints the address, tries to open it, and waits, so it works over SSH.
- The listener asked for no new Last.fm tests: existing tests are adapted to the new flow, none added.

## Tasks

1. **Online crate**: `approval_asked(client, application) -> Result<LastfmApproving>` (`address()`, `approved(cancelled) -> Result<Option<Session>>`, polling `ASKED_EVERY` 3 s, code 14 waits), `signed_in` removed, the signing test rewritten over `auth.getToken`.
2. **Library seam**: `LastfmSignIn { key, secret }`, `trait LastfmApproval { address, approved }`, `Scrobblers::lastfm_approval`.
3. **Binary**: `resonate lastfm [--forget]` over the approval (print, open with `xdg-open` / `explorer`, wait with the first interrupt cancelling, store the session); `--user`, the unechoed prompt and its errors removed; `ByToken` answers the approval.
4. **Window**: the Last.fm group keeps the key and secret fields alone; *Sign in to Last.fm* opens the page (`cx.open_url`) and waits beside *Open the page again* and *Stop*; the driven test follows the new seam.
5. **Phones' boundary**: `authorize_lastfm(key, secret) -> address`, `finish_lastfm_authorization() -> Option<LastfmSignedIn { name, session }>`, `cancel_lastfm_authorization()`, attempt-numbered; `lastfm_sign_in` removed.
6. **Android / iOS**: the scrobbling page drops user and password and opens the page in the browser, waiting with *Cancel* (not compiled here).
7. **Rules**: `online.md`'s *Last.fm*, `binary.md`, `ui.md`, `mobile.md`, `android.md`, `ios.md`, the man page's `lastfm-session` entry.
