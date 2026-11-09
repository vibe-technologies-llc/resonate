# Jellyfin Quick Connect Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Sign in to a Jellyfin server by approving a six-character code in a Jellyfin app already signed in, on the desktop window, the terminal and both phones, keeping the session it yields.

**Architecture:** `resonate-jellyfin` gains the flow (`QuickConnect`: offered, begun, approved) and a `Server` that may carry a kept session in place of a password. The binary keeps the session in three new settings keys and offers `resonate jellyfin`; the window gets a `SignsInToJellyfin` seam beside TIDAL's; `resonate-mobile` widens `DownloadAccount` and exposes begin/finish/cancel as TIDAL's device sign-in does; Android and iOS draw the code.

**Tech Stack:** Rust (ureq through `resonate-fetch`, serde), GPUI, UniFFI, Kotlin/Compose, SwiftUI.

**Spec:** `docs/superpowers/specs/2026-11-08-sign-in-design.md`, section 1 and *For all three*. Code map: `.superpowers/sdd/map-jellyfin-lastfm.md`.

## Global Constraints

- Flow (Jellyfin 10.8+): `GET QuickConnect/Enabled`; `POST QuickConnect/Initiate` (on `405`, `GET`) with the `MediaBrowser` authorization header, answering `Code` and `Secret`; poll `GET QuickConnect/Connect?secret=…` until `Authenticated`; `POST Users/AuthenticateWithQuickConnect` with `{"Secret"}` answers `AccessToken` and `User.Id`.
- A lapse, a cancel and a refusal end with no token (`AuthorizationLapsed`, `Ok(None)`, `AuthorizationDenied`), as TIDAL's device flow does; Resonate stops waiting after `A_QUICK_CONNECT_LAPSES_AFTER` (10 minutes).
- Kept: access token, user id and the device id it was issued to (the per-instance random UUID becomes a kept one for this case), as new settings `jellyfin-token`, `jellyfin-user-id`, `jellyfin-device`; every new value prints `<withheld>`.
- A token and a password never both stand: a Quick Connect sign-in clears `jellyfin-password` (and writes the user's name into `jellyfin-user`); a password typed afterwards clears the kept token, user id and device.
- A kept token refused (401) is the provider away under `ProviderOp::SignIn`, never another sign-in.
- Ready = an address and either user and password, or a kept token (desktop `Accounts`, phones `Readiness` / `is_ready`).
- Emby has no Quick Connect: not offered where the address reads as Emby (`flavour_of`: ends `/emby`) or `QuickConnect/Enabled` is not answered `true`; a server with it off says so and offers the password.
- Phones' stored account records gain token, user id and device id as sealed fields; existing records read on.
- No comments in Rust; errors per `errors.md`; `rust-formatter`.

## Review Focus

- A code approved after Resonate stopped waiting (cancelled, lapsed) must not sign in later: the poll ends on the cancel and keeps nothing (Task 2).
- A server answering `Enabled` with something other than `true`/`false` (an HTML login page from a proxy) is "not offered", never a crash or a sign-in attempt (Task 2).
- A kept token from one server must never be sent to another address typed later: changing `jellyfin` clears the kept session (Task 3).
- The secret and token never reach a log line or a URL in a log (Task 2: `told` already strips addresses).
- An old phone record (no token fields) still reads, and a password account on it behaves as before (Tasks 6–7).

---

### Task 1: A server signed in by a kept session

**Files:** Modify `crates/providers/jellyfin/src/lib.rs`, `tests/server.rs`.

**Interfaces:**
- Produces: `pub struct KeptSession { pub token: String, pub user: String, pub device: String }` (Debug withholds token); `Server { url, user, password, kept: Option<KeptSession> }`. `Jellyfin::at` with `kept` uses `kept.device` as its device id and seeds the session (`root` = the address as given, `Flavour::Jellyfin`); `asked` on a 401 with a kept session forgets it and answers `Error::Unwelcome` under `ProviderOp::SignIn`, never signing in by name.
- [ ] Tests: `a_kept_session_is_asked_with_its_token_and_device_and_never_signs_in` (fake records no `AuthenticateByName`, the browse carries `api_key=<token>`, authorization names the kept device); `a_kept_token_turned_away_is_the_server_away_not_a_new_sign_in`; `a_kept_sessions_token_never_reaches_debug`.
- [ ] Implement, run `cargo test -p resonate-jellyfin`, commit.

### Task 2: The Quick Connect flow

**Files:** Create `crates/providers/jellyfin/src/quick_connect.rs`; modify `lib.rs`, `served.rs`, `tests/server.rs` (new tests file `tests/quick_connect.rs` sharing the fake through `tests/support` if needed).

**Interfaces:**
- Produces: `pub struct QuickConnect` built by `QuickConnect::at(address: &str) -> Self`, `.asked_every(Duration)`, `.lapsing_after(Duration)` (tests); `fn offered(&self) -> Result<bool>` (Emby address or a non-`true` answer → `false`); `fn begun(&self) -> Result<Approving>`; `fn approved(&self, approving: &Approving, cancelled: &dyn Fn() -> bool) -> Result<Option<SignedIn>>`. `pub struct Approving { pub code: String, secret, device }` (Debug withholds secret); `pub struct SignedIn { pub session: KeptSession, pub name: String }`. Lapse (Connect `404`, or past `lapsing_after`) → `Error::AuthorizationLapsed`; a `401`/`403` at Initiate → `Error::AuthorizationDenied`; unreached and 5xx asked again.
- Constants: `QUICK_CONNECT_ASKED_EVERY` 5 s, `A_QUICK_CONNECT_LAPSES_AFTER` 10 min.
- [ ] Tests (fake server): `a_code_approved_elsewhere_signs_in_and_yields_the_session_the_device_was_issued` (Initiate carries the authorization header; AuthenticateWithQuickConnect body holds the secret; the session's device is the one Initiate named); `a_code_never_approved_lapses_and_keeps_nothing`; `a_cancel_ends_the_wait_and_keeps_nothing`; `quick_connect_is_not_offered_by_emby_or_a_server_with_it_off_or_one_answering_a_page`; `an_older_server_is_begun_with_get`; `the_secret_never_reaches_debug`.
- [ ] Implement, `cargo test -p resonate-jellyfin`, clippy, commit.

### Task 3: The binary keeps the session, and `resonate jellyfin`

**Files:** Modify `crates/resonate/src/{config.rs, error.rs, settings.rs, providers.rs, cli.rs, main.rs}`, create `crates/resonate/src/jellyfin.rs`, `packaging/resonate-config.toml.5`.

**Interfaces:**
- `ConfigKey::{JellyfinToken, JellyfinUserId, JellyfinDevice}` (`jellyfin-token`, `jellyfin-user-id`, `jellyfin-device`), `Config` fields, Debug withheld, man entries. `JellyfinServer` gains `kept: Option<KeptSession>`; `fn jellyfin(url, user, password, token, user_id, device)` ready with password or a whole kept session.
- `resonate jellyfin [--forget]`: `OnlineOff`, `NoJellyfinServer`, `QuickConnectOff` (not offered) before anything; prints the code and "Approve this code in a Jellyfin app you are signed in to (Settings → Quick Connect)"; the first interrupt cancels (`JellyfinSignInCancelled`); signed in, one `config::edit` stores the three keys and the user's name and clears `jellyfin-password`. `--forget` clears the three keys.
- `Setting::JellyfinPassword` written non-blank also clears the three keys; `Setting::Jellyfin` (address) changed clears them too.
- [ ] Tests: `a_jellyfin_account_is_ready_with_a_password_or_a_kept_session`; `a_quick_connect_sign_in_keeps_the_session_and_lets_go_of_the_password` (fake server, config file); `a_jellyfin_sign_in_with_online_off_or_no_server_is_refused_before_anything_is_asked`; `a_password_typed_or_an_address_changed_lets_go_of_a_kept_session`; Debug test extended; man page test passes.
- [ ] Implement, tests, clippy matrix, commit.

### Task 4: The window's Jellyfin group

**Files:** Modify `crates/resonate-ui/src/{settings.rs, app.rs, lib.rs, views/settings/subsonic.rs, views/root.rs, driven.rs}`, `crates/resonate/src/{providers.rs, main.rs}`.

**Interfaces:**
- `pub trait SignsInToJellyfin: Send + Sync { fn offered(&self, address: &str) -> bool; fn begun(&self, address: &str) -> resonate_providers::Result<JellyfinApproving>; fn approved(&self, approving: &JellyfinApproving, stopped: &AtomicBool) -> resonate_providers::Result<Option<JellyfinSignedIn>>; }` with gpui-free records `JellyfinApproving { code, .. }`, `JellyfinSignedIn { token, user_id, device, name }` (Debug withheld). `Lookups::signs_in_to_jellyfin`.
- Group: beside the address, *Sign in with Quick Connect* (`jellyfin-quick-connect`) where offered (asked on the background executor when the address is given or the pane opens; not offered → a note *This server has Quick Connect off; sign in with the password*); while waiting the code as a `kit::figure`, the sentence, *Cancel* (`jellyfin-quick-connect-cancel`); signed in: the user field holds the name, the password is emptied, the three settings written, *Signed in as <name>*, `the_sources_moved`.
- [ ] Driven tests with a fake seam: `a_jellyfin_code_approved_elsewhere_signs_in_and_empties_the_password`; `a_jellyfin_sign_in_cancelled_keeps_nothing`; `a_server_without_quick_connect_offers_the_password_alone`.
- [ ] Implement, `cargo test -p resonate-ui`, binary adapter + test, commit.

### Task 5: The phones' boundary

**Files:** Modify `crates/resonate-mobile/src/{downloads.rs, served.rs}` (+ error mapping).

**Interfaces:** `DownloadAccount` gains `token`, `user_id`, `device` (`String`, empty when none); `reached()` builds the kept session; `is_ready` per the readiness rule. `begin_jellyfin_quick_connect(server: String) -> Result<JellyfinQuickConnect { code }>`, `finish_jellyfin_quick_connect() -> Result<Option<JellyfinSession { token, user_id, device, name }>>` (blocks), `cancel_jellyfin_quick_connect()`, attempt-numbered as TIDAL's.
- [ ] Tests (loopback fake): approved, cancelled, an account ready with a kept session.
- [ ] Implement, `cargo test -p resonate-mobile`, Android clippy check where possible, commit.

### Task 6: Android

**Files:** `android/app/src/main/java/org/resonate/android/{ProviderAccountBook.kt, ProviderAccountRecords.kt, LibraryFeatures.kt, ui/settings/ProviderAccountScreen.kt}`, their tests.

- Records version `3`: the three fields sealed; a version `2` record reads with them empty. Readiness: Jellyfin ready with server + (user + password or token). Screen: *Sign in with Quick Connect* beside the server field for Jellyfin, the code, *Cancel*; signed in writes the account (password emptied).
- [ ] JVM tests for the record migration and readiness; CI's Android job compiles the screen (no SDK on this machine).

### Task 7: iOS

**Files:** `ios/Sources/ResonateLogic/{AccountStore.swift, Downloads.swift}`, `ios/Sources/Resonate/{DownloadViews.swift, Features.swift}`, logic tests.

- `KeptProviderAccount` decodes the three fields as optional; readiness as Android's; the view's Jellyfin fields gain the button, code and cancel. Not compiled here (no Swift toolchain): written to the TIDAL sign-in's shape.

### Task 8: Rules and docs

`providers.md` (Jellyfin section), `binary.md` (`resonate jellyfin`, the keys), `ui.md`, `mobile.md`, `android.md`, `ios.md`; `CLAUDE.md` command list.
