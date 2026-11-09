# YouTube Music Sign-in Window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A *Sign in to YouTube Music* window, owned by a separate `resonate-signin` program on wry, that writes a cookie jar of Resonate's own which `youtube-music-cookies` then names.

**Architecture:** `crates/resonate-signin` is a library (the jar, the done test, the exit-status protocol; no browser engine) and, under its default `window` feature, the `resonate-signin` executable linking wry + tao. The `resonate` binary depends on the library with `default-features = false`, finds the executable, runs it as a child and maps its exit status; the window reaches that through a new `SignsInToYoutubeMusic` seam on `Lookups`, so `resonate-ui` names neither crate.

**Tech Stack:** wry 0.55.1 (WebKitGTK 4.1 on Linux, WebView2 on Windows), tao 0.35.3, clap, thiserror, `resonate_core::writer::Held`.

**Spec:** `docs/superpowers/specs/2026-11-08-sign-in-design.md`, section 3 and *For all three*.

## Global Constraints

- The `resonate` binary (window included) never links a browser engine: the layering job refuses `wry`, `tao`, `webkit2gtk`, `webview2-com` from `resonate`'s tree, with and without default features.
- `resonate-signin` reaches `resonate-core` alone among workspace crates; it may not reach the engine or the UI.
- Sign-in URL: `https://accounts.google.com/ServiceLogin?service=youtube&continue=https://music.youtube.com/`; WebKitGTK's default user agent (none set).
- Done = the page is on `music.youtube.com` and the jar holds `LOGIN_INFO` and `SAPISID` under `.youtube.com`. Closing the window first is a cancel and keeps nothing.
- Jar: in the data folder (`config::data_dir()`), owner-only (`0600` on Unix), written whole then renamed into place. Profile: also in the data folder, never a listener's browser.
- Signing in points `youtube-music-cookies` at the jar (a browser value is not kept); signing out deletes jar and profile and clears the setting.
- No secret in a log, `Debug` or URL: cookie values, the jar's path and contents print `<withheld>`.
- `resonate-signin` is optional in every package: Arch split package in `optdepends`, Fedora subpackage `Recommends`, Debian `Recommends`; the Windows zip and installer carry it; the Flatpak does not.
- No new settings key; no stored format's `V1` edited. No comments in Rust (`rust-style.md`); `rust-formatter`, never `cargo fmt`.

## Review Focus

- A cookie whose name, value, domain or path holds a tab, CR or LF would forge a jar line: such a cookie is left out of the jar (Task 1).
- A Google sign-in that lands signed out on `music.youtube.com` (consent page, a brand account chooser left half way) must not close the window as signed in: only `LOGIN_INFO` + `SAPISID` under `.youtube.com` count; `.google.com` cookies alone do not (Task 1).
- Two sign-ins at once (pane pressed while `resonate ytmusic sign-in` runs) must not interleave one jar: the staging file is held, the second write refused (Task 1).
- A cancelled or failed sign-in leaves `youtube-music-cookies` as it was, a browser name included (Task 4).
- *Sign out* when the setting names the listener's own `cookies.txt` or a browser deletes nothing of theirs (Task 4).

---

### Task 1: The jar, the done test and the exit protocol (library)

**Files:**
- Create: `crates/resonate-signin/Cargo.toml`, `src/lib.rs`, `src/jar.rs`, `src/outcome.rs`, `src/error.rs`
- Modify: `Cargo.toml` (member, `[workspace.dependencies]`: `resonate-signin`, `wry = { version = "0.55.1", default-features = false, features = [...] }`, `tao = { version = "0.35.3", default-features = false, features = [...] }`)

**Interfaces:**
- Produces:
  - `pub struct Cookie { pub domain: String, pub path: String, pub secure: bool, pub http_only: bool, pub expires: Option<u64>, pub name: String, pub value: String }`, `Debug` printing `value: <withheld>`.
  - `pub fn kept(cookies: Vec<Cookie>) -> Vec<Cookie>`: those under `youtube.com` or `google.com` (the domain itself or a subdomain, leading dot ignored) whose fields hold no tab, CR or LF.
  - `pub fn signed_in(cookies: &[Cookie]) -> bool`: `LOGIN_INFO` and `SAPISID` present with domain `.youtube.com` or `youtube.com`.
  - `pub fn netscape(cookies: &[Cookie]) -> String`: `# Netscape HTTP Cookie File\n` then one line each: `#HttpOnly_` prefix where `http_only`, domain, `TRUE` where the domain starts with `.` else `FALSE`, path, `TRUE`/`FALSE` secure, expiry seconds (`0` for a session cookie), name, value, tab-separated.
  - `pub fn written(jar: &Path, cookies: &[Cookie]) -> Result<()>`: creates the parent folder; stages `<jar>.staged` under `Held::taken_over`, mode `0o600` on Unix before any byte, writes, `sync_all`, renames over `jar`.
  - `pub enum Outcome { SignedIn, Closed, Failed { code: Option<i32> } }`, `pub const SIGNED_IN: i32 = 0`, `pub const CLOSED: i32 = 3`, `Outcome::of(status: ExitStatus) -> Outcome`, `Outcome::code(self) -> i32`.
  - `pub enum Error { Jar { op: JarOp, #[source] source: io::Error } }`, `JarOp::{MakeFolder, Stage, Narrow, Write, Sync, PutInPlace}`; guard test `size_of::<Error>() <= 128`.

- [ ] **Step 1: Write the failing tests** in `jar.rs`/`outcome.rs` `#[cfg(test)]`:
  - `a_jar_reads_as_netscape_with_http_only_cookies_marked`: two cookies (one `http_only`, session) → exact expected text.
  - `only_youtube_and_google_cookies_are_kept_and_a_lookalike_domain_is_not`: `.youtube.com`, `accounts.google.com` kept; `.doubleclick.net`, `evil-youtube.com`, `youtube.com.evil.net` left out.
  - `a_cookie_that_would_break_its_line_is_left_out`: value `a\tb`, value `a\nb`, name `a\rb` → none kept.
  - `google_cookies_alone_are_not_a_youtube_sign_in`: `SAPISID` under `.google.com` only → false; `LOGIN_INFO` + `SAPISID` under `.youtube.com` → true.
  - `a_jar_is_written_whole_owner_only_and_into_a_folder_made_for_it` (tempdir, missing parent): content equals `netscape`, mode `0o600` on Unix, no `.staged` left.
  - `a_jar_another_sign_in_is_writing_is_not_written_over`: a live `Held::taken_over` on `<jar>.staged` → `written` is `Err(Error::Jar { op: JarOp::Stage, .. })` with `WouldBlock`, jar untouched.
  - `a_cookies_value_never_reaches_debug`.
  - `each_exit_status_reads_as_its_outcome`: 0 → `SignedIn`, 3 → `Closed`, 1 → `Failed { code: Some(1) }`.
- [ ] **Step 2:** `cargo test -p resonate-signin --no-default-features` → fails to compile.
- [ ] **Step 3:** Implement; the library builds without the `window` feature (no wry, no tao).
- [ ] **Step 4:** `cargo test -p resonate-signin --no-default-features` → pass; `cargo clippy -p resonate-signin --no-default-features --all-targets -- -D warnings`.
- [ ] **Step 5:** Commit.

### Task 2: The window (Linux spike, then Windows cross-check)

**Files:**
- Create: `crates/resonate-signin/src/main.rs` (`required-features = ["window"]`), `src/window.rs` (`#[cfg(feature = "window")]`)
- Modify: `.github/workflows/ci.yml` (`webkit2gtk-4.1` in `ARCH_PACKAGES`; layering refusals), `crates/resonate-signin/Cargo.toml`

**Interfaces:**
- Consumes: Task 1's `kept`, `signed_in`, `written`, `Outcome`.
- Produces: `resonate-signin --jar <FILE> --profile <FOLDER>` exits `SIGNED_IN` once the jar is written, `CLOSED` when the window is closed first, any other code on failure (logged through `tracing` to stderr, the path withheld).

- [ ] **Step 1:** `main.rs`: clap args; `#![cfg_attr(windows, windows_subsystem = "windows")]`. `window.rs`: tao event loop + window *Sign in to YouTube Music*; `WebContext::new(Some(profile))`; `WebViewBuilder::new_with_web_context` loading the sign-in URL (on Linux `build_gtk(window.default_vbox())`); on every page load *Finished* and once a second while `webview.url()` has host `music.youtube.com`, read `webview.cookies()`, convert `cookie::Cookie` → `jar::Cookie` (domain, path, secure, http_only, `expires_datetime` as Unix seconds, session `None`), `kept`, and where `signed_in`, `written` and exit `SIGNED_IN`; `CloseRequested` exits `CLOSED`.
- [ ] **Step 2:** `cargo build -p resonate-signin` and clippy with default features.
- [ ] **Step 3 (hand check, Linux):** launch on the live session (the `run-ui` skill's environment recipe) with `--jar` and `--profile` in the scratchpad; the listener signs in; then `yt-dlp --ignore-config --cookies <jar> --flat-playlist -J 'https://music.youtube.com/playlist?list=LM'` lists *Liked songs* and `yt-dlp --ignore-config --cookies <jar> -F 'https://music.youtube.com/watch?v=9RfVp-GhKfs'` lists 774 or 141. A failure here ends the window on Linux (spec's *Where wry falls short*).
- [ ] **Step 4 (Windows):** `RUSTFLAGS= cargo xwin check --target x86_64-pc-windows-msvc -p resonate-signin`; the WebView2 sign-in is a hand check on a Windows machine, recorded as not yet made until the listener makes it.
- [ ] **Step 5:** CI: `webkit2gtk-4.1` in `ARCH_PACKAGES`; layering `refuse resonate wry tao webkit2gtk webview2-com`, `refuse_headless resonate wry tao webkit2gtk webview2-com`, `only resonate-signin resonate-core`. Commit.

### Task 3: The seam and the command line

**Files:**
- Create: `crates/resonate/src/signing_in.rs` (`online` feature)
- Modify: `crates/resonate/Cargo.toml` (`resonate-signin = { workspace = true, default-features = false }` under `online`), `cli.rs` (`YtmusicArgs::SignIn`, `YtmusicArgs::SignOut`), `ytmusic.rs` (`run` arms), `error.rs`, `providers.rs` (`stop_the_tools` also ends a sign-in child), `main.rs`

**Interfaces:**
- Produces (binary):
  - `pub fn the_program() -> Option<PathBuf>`: `resonate-signin` (`.exe` on Windows) beside `env::current_exe()`, else the first on `PATH` (`env::split_paths`).
  - `pub struct Kept { pub jar: PathBuf, pub profile: PathBuf }`, `Kept::in_the_data_folder() -> Result<Kept>`: `data_dir()?.join("youtube-music-cookies.txt")`, `data_dir()?.join("youtube-music-sign-in")`.
  - `pub fn sign_in(program: &Path, kept: &Kept) -> Result<()>`: spawns `--jar --profile` (no console window on Windows), holds the child in a process-wide slot `stop_the_tools` kills, waits; `Outcome::SignedIn` → `Ok`, `Closed` → `Error::YoutubeMusicSignInClosed`, `Failed { code }` → `Error::YoutubeMusicSignInFailed { code }`, a spawn error → `Error::YoutubeMusicSignInNotStarted { source }`.
  - `pub fn sign_out(kept: &Kept) -> Result<()>`: removes jar and profile, `NotFound` being no failure; `Error::YoutubeMusicSignOut { op: SignOutOp::{Jar, Profile}, source }`.
  - `pub fn names_the_jar(setting: &str, kept: &Kept) -> bool`.
  - `Error::NoSignInProgram` (says to install `resonate-signin`).
- `resonate ytmusic sign-in`: `OnlineOff`, `YoutubeMusicOff`, `NoSignInProgram` before anything starts; signed in → `config::store` `youtube-music-cookies` = jar path, then prints `connection_said(check)`. `resonate ytmusic sign-out`: clears the key only where `names_the_jar`, deletes jar and profile either way.

- [ ] **Step 1: Failing tests** (`signing_in.rs`, `ytmusic.rs`):
  - `the_program_beside_resonate_is_found_before_one_on_the_path` (temp folders as stand-ins; the finder takes the folder and `PATH` as arguments).
  - `a_sign_in_with_online_or_youtube_music_off_or_no_program_is_refused_before_anything_starts`.
  - `each_ending_of_the_sign_in_program_is_its_own_answer`: fake programs (a shell script on Unix exiting 0 after writing the jar, 3, 1) → `Ok`, `YoutubeMusicSignInClosed`, `YoutubeMusicSignInFailed { code: Some(1) }`.
  - `a_sign_in_closed_or_failed_leaves_the_setting_as_it_was` (config naming `firefox`).
  - `signing_out_deletes_only_what_the_window_kept` (`cookies.txt` of the listener's survives; setting naming it is left).
- [ ] **Step 2:** `cargo test -p resonate --no-default-features --features online signing_in ytmusic` → fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Tests pass; clippy for every feature set listed in `CLAUDE.md`.
- [ ] **Step 5:** Commit.

### Task 4: The window's YouTube Music group

**Files:**
- Modify: `crates/resonate-ui/src/settings.rs` (seam), `app.rs` (`Lookups::signs_in_to_youtube_music`, `ResonateApp` field), `lib.rs` (re-export), `views/settings/youtube_music.rs`, `driven.rs`; `crates/resonate/src/signing_in.rs` (adapter, `ui` + `online`), `main.rs`

**Interfaces:**
- Produces (`resonate-ui`):
  - `pub enum YoutubeMusicSignIn { SignedIn { cookies: String }, Closed, ProgramMissing, Failed }`
  - `pub trait SignsInToYoutubeMusic: Send + Sync { fn is_offered(&self) -> bool; fn sign_in(&self) -> YoutubeMusicSignIn; fn signs_out(&self, cookies: &str) -> bool; fn names_the_window_jar(&self, cookies: &str) -> bool; }` (`signs_out` answers whether the setting is to be cleared).
- Pane: where the seam `is_offered`, the action row leads with *Sign in to YouTube Music* (*Sign in again* where `names_the_window_jar`), greyed like *Check connection* without Online or the switch, *Signing in…* while waiting; signed in, the value goes into the Cookies field, the global and `Setting::YoutubeMusicCookies`, `the_sources_moved`, then the check runs and toasts. *Sign out* shows where `names_the_window_jar`. Not offered: a note *Install resonate-signin to sign in from here*. Closed: toast *Sign-in closed, nothing kept*; failed: *The sign-in window stopped*.

- [ ] **Step 1: Failing driven tests** (`views/settings/youtube_music.rs` or `driven.rs`, a fake seam):
  - `signing_in_to_youtube_music_names_the_jar_and_checks_the_account`.
  - `a_sign_in_closed_keeps_the_browser_the_settings_named`.
  - `sign_out_is_offered_only_for_what_the_window_kept_and_clears_it`.
  - `without_the_sign_in_program_the_pane_says_to_install_it`.
- [ ] **Step 2:** `cargo test -p resonate-ui <names>` → fail.
- [ ] **Step 3:** Implement, plus the binary adapter over Task 3 and `main.rs` wiring.
- [ ] **Step 4:** `cargo test -p resonate-ui`, `cargo test -p resonate`, clippy matrix.
- [ ] **Step 5:** Commit.

### Task 5: Packaging, rules and docs

**Files:**
- Modify: `packaging/PKGBUILD`, `packaging/release/PKGBUILD` (split `resonate-signin`, `depends=webkit2gtk-4.1`, main `optdepends`), `packaging/resonate.spec` (subpackage, `Recommends`), `packaging/deb/build.sh` (second `.deb`, `Recommends`), `packaging/windows/build.ps1` and `resonate.iss` (build, sign, ship the exe), `.github/workflows/release.yml` where an asset list needs it, `packaging/resonate-config.toml.5` (`youtube-music-cookies` names the jar after a sign-in), `CLAUDE.md` (tree, thirty-seven crates, commands), `AGENTS.md` only if its pointers move, `.claude/rules/providers.md`, `binary.md`, `ui.md`, `dependencies.md`, `build.md`, `packaging.md`, `docs/` roadmap entries the work closes.

- [ ] **Step 1:** Packaging edits; `bash -n packaging/deb/build.sh`; `makepkg --printsrcinfo` on the PKGBUILD.
- [ ] **Step 2:** Rules and docs, each saying what landed.
- [ ] **Step 3:** Full check: `cargo build --workspace`, `cargo test --workspace --exclude resonate-ui --no-default-features`, `cargo test -p resonate-ui`, clippy matrix, `rust-formatter --check`, the layering job's commands run locally.
- [ ] **Step 4:** Commit.
